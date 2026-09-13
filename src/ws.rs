use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::session::{Registry, Role, Session};
use crate::wire::{ClientMsg, Frame, ServerMsg};

pub struct Join {
    pub id: String,
    pub token: Option<String>,
    /// Anonymous, browser generated, and only ever used to keep one voter from
    /// filling the tally. It is not an identity and not a defense against a
    /// determined cheat.
    pub who: String,
}

pub async fn serve(socket: WebSocket, registry: Registry, join: Join) {
    let Join { id, token, who } = join;
    let role = token
        .as_deref()
        .map(|t| registry.role(&id, t))
        .unwrap_or(Role::Viewer);
    // Notes, tallies and the deck source follow the ability to edit, so a
    // co-host sees what they need to write the next question.
    let is_owner = role.edits();
    let Some(mut rx) = registry.subscribe(&id) else {
        return;
    };
    let (mut sink, mut stream) = socket.split();

    // One lock for the whole opening state, so a socket is never hydrated from
    // a snapshot of one moment and a leaderboard of another.
    let Some(opening) = registry.with(&id, |s| s.catch_up(is_owner)) else {
        return;
    };
    for msg in &opening {
        if send(&mut sink, msg, is_owner).await.is_err() {
            return;
        }
    }
    // A full room closes the socket. A viewer who silently saw nothing would
    // look like a broken app rather than a full one.
    if registry.with_mut(&id, Session::join).flatten().is_none() {
        return;
    }

    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                match outgoing {
                    Ok(frame) => {
                        if send_frame(&mut sink, &frame, is_owner).await.is_err() {
                            break;
                        }
                    }
                    // A phone on bar wifi falls behind a burst of reactions.
                    // Resend the state it missed instead of closing on it.
                    Err(RecvError::Lagged(_)) => {
                        if resync(&mut sink, &registry, &id, is_owner).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            incoming = stream.next() => {
                let Some(Ok(frame)) = incoming else { break };
                let Message::Text(text) = frame else { continue };
                let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else {
                    continue;
                };
                handle(&registry, &id, token.as_deref(), role, &who, msg);
            }
        }
    }

    registry.with_mut(&id, Session::leave);
}

/// Every branch re-checks the token inside the registry, so a forged frame from
/// a viewer changes nothing.
fn handle(
    registry: &Registry,
    id: &str,
    token: Option<&str>,
    role: Role,
    who: &str,
    msg: ClientMsg,
) {
    match msg {
        ClientMsg::Goto { index } => {
            if let Some(token) = token {
                registry.with_mut(id, |s| s.goto(token, index));
            }
        }
        // The reveal also moves the board, and both leave under the one lock.
        ClientMsg::Reveal { slide } => {
            if let Some(token) = token {
                registry.with_mut(id, |s| s.reveal(token, slide));
            }
        }
        ClientMsg::SetName { name } => {
            registry.with_mut(id, |s| s.set_name(who, &name));
        }
        ClientMsg::React { kind } => {
            registry.with_mut(id, |s| s.react(who, kind));
        }
        ClientMsg::Ask { text } => {
            registry.with_mut(id, |s| s.ask(who, &text));
        }
        ClientMsg::Upvote { question } => {
            registry.with_mut(id, |s| s.upvote(who, question));
        }
        ClientMsg::Answered { question } => {
            registry.with_mut(id, |s| s.mark_answered(role, question));
        }
        // Anyone in the room may vote, the presenter included.
        ClientMsg::Answer { slide, options } => {
            registry.with_mut(id, |s| s.answer(slide, who, &options));
        }
    }
}

/// Everything a socket needs to be correct again after missing messages.
async fn resync<S>(sink: &mut S, registry: &Registry, id: &str, is_owner: bool) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let Some(state) = registry.with(id, |s| s.catch_up(is_owner)) else {
        return Ok(());
    };
    for msg in &state {
        send(sink, msg, is_owner).await?;
    }
    Ok(())
}

/// A broadcast arrives already serialized, so this only picks the copy that
/// belongs to this socket.
async fn send_frame<S>(sink: &mut S, frame: &Frame, is_owner: bool) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let Some(text) = frame.for_socket(is_owner) else {
        return Ok(());
    };
    sink.send(Message::Text(text.to_string().into()))
        .await
        .map_err(|_| ())
}

/// The opening messages go to one socket, so they are built for it directly.
async fn send<S>(sink: &mut S, msg: &ServerMsg, is_owner: bool) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let payload = if is_owner {
        Some(msg.clone())
    } else {
        msg.redacted()
    };
    let Some(payload) = payload else {
        return Ok(());
    };
    let text = serde_json::to_string(&payload).map_err(|_| ())?;
    sink.send(Message::Text(text.into())).await.map_err(|_| ())
}
