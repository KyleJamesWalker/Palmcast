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
    // Notes, tallies and the deck source follow the ability to edit or drive,
    // so a co-host sees what they need to write the next question and a speaker
    // handed the controls sees their own notes.
    let mut is_staff = staff_now(&registry, &id, token.as_deref());
    let Some(mut rx) = registry.subscribe(&id) else {
        return;
    };
    let (mut sink, mut stream) = socket.split();

    // One lock for the whole opening state, so a socket is never hydrated from
    // a snapshot of one moment and a leaderboard of another.
    let Some(opening) = registry.with(&id, |s| s.catch_up(is_staff)) else {
        return;
    };
    for msg in &opening {
        if send(&mut sink, msg, is_staff).await.is_err() {
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
                        if send_frame(&mut sink, &frame, is_staff).await.is_err() {
                            break;
                        }
                        // The host just handed the controls somewhere. If that
                        // was to or from this socket, what it may see changed,
                        // and it needs the state it was not being sent.
                        if frame.rerole {
                            let now = staff_now(&registry, &id, token.as_deref());
                            if now != is_staff {
                                is_staff = now;
                                if resync(&mut sink, &registry, &id, is_staff).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    // A phone on bar wifi falls behind a burst of reactions.
                    // Resend the state it missed instead of closing on it.
                    Err(RecvError::Lagged(_)) => {
                        if resync(&mut sink, &registry, &id, is_staff).await.is_err() {
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
                handle(&registry, &id, token.as_deref(), &who, msg);
            }
        }
    }

    registry.with_mut(&id, Session::leave);
}

/// What this socket may see right now.
///
/// Read fresh rather than captured, because the host can hand the controls over
/// while a socket is open.
fn staff_now(registry: &Registry, id: &str, token: Option<&str>) -> bool {
    let role = token.map(|t| registry.role(id, t)).unwrap_or(Role::Viewer);
    role.edits() || role.drives()
}

/// Every branch re-checks the token inside the registry, so a forged frame from
/// a viewer changes nothing. The role is read at the moment the frame lands,
/// never captured when the socket opened: a speaker handed the controls must
/// start driving without reconnecting, and one handed them back must stop.
fn handle(registry: &Registry, id: &str, token: Option<&str>, who: &str, msg: ClientMsg) {
    let token = token.unwrap_or("");
    match msg {
        ClientMsg::Goto { index, step } => {
            registry.with_mut(id, |s| s.goto(token, index, step));
        }
        // The reveal also moves the board, and both leave under the one lock.
        ClientMsg::Reveal { slide } => {
            registry.with_mut(id, |s| s.reveal(token, slide));
        }
        ClientMsg::Stage { talk } => {
            registry.with_mut(id, |s| s.stage(s.role_of(token), talk));
        }
        ClientMsg::Hand { talk } => {
            registry.with_mut(id, |s| s.hand(s.role_of(token), talk));
        }
        ClientMsg::Submissions { open } => {
            registry.with_mut(id, |s| s.set_submissions(s.role_of(token), open));
        }
        ClientMsg::Reorder { talk, index } => {
            registry.with_mut(id, |s| s.reorder(s.role_of(token), talk, index));
        }
        ClientMsg::Drop { talk, note } => {
            registry.with_mut(id, |s| s.drop_talk(s.role_of(token), talk, &note));
        }
        ClientMsg::Restore { talk } => {
            registry.with_mut(id, |s| s.restore_talk(s.role_of(token), talk));
        }
        ClientMsg::Remove { talk } => {
            registry.with_mut(id, |s| s.remove_talk(s.role_of(token), talk));
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
            registry.with_mut(id, |s| s.mark_answered(s.role_of(token), question));
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
