use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::session::{Registry, Role};
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

    if let Some(snapshot) = registry.snapshot(&id)
        && send(&mut sink, &snapshot, is_owner).await.is_err()
    {
        return;
    }
    // A joiner needs the questions already on the floor, not just the ones
    // asked after they arrived.
    if let Some(questions) = registry.questions(&id)
        && send(&mut sink, &questions, is_owner).await.is_err()
    {
        return;
    }
    if let Some(scores) = registry.scores(&id)
        && send(&mut sink, &scores, is_owner).await.is_err()
    {
        return;
    }
    // An answer the presenter already opened is public, so it goes to whoever
    // has just arrived as well.
    for reveal in registry.reveals(&id) {
        if send(&mut sink, &reveal, is_owner).await.is_err() {
            return;
        }
    }
    // A round can already be under way. Only the presenter is told, because the
    // room seeing the split form is the thing the tally is withheld for.
    if is_owner {
        for tally in registry.tallies(&id) {
            if send(&mut sink, &tally, is_owner).await.is_err() {
                return;
            }
        }
    }
    // A full room closes the socket. A viewer who silently saw nothing would
    // look like a broken app rather than a full one.
    let Some(count) = registry.join(&id) else {
        return;
    };
    registry.broadcast(&id, count);

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

    if let Some(count) = registry.leave(&id) {
        registry.broadcast(&id, count);
    }
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
            if let Some(token) = token
                && let Some(moved) = registry.goto(id, token, index)
            {
                registry.broadcast(id, moved);
            }
        }
        ClientMsg::Reveal { slide } => {
            if let Some(token) = token
                && let Some(revealed) = registry.reveal(id, token, slide)
            {
                registry.broadcast(id, revealed);
                // The board only changes when an answer opens, so it rides
                // along with the reveal rather than on a timer.
                if let Some(scores) = registry.scores(id) {
                    registry.broadcast(id, scores);
                }
            }
        }
        ClientMsg::SetName { name } => {
            if let Some(scores) = registry.set_name(id, who, &name) {
                registry.broadcast(id, scores);
            }
        }
        ClientMsg::React { kind } => {
            if let Some(react) = registry.react(id, who, kind) {
                registry.broadcast(id, react);
            }
        }
        ClientMsg::Ask { text } => {
            if let Some(list) = registry.ask(id, who, &text) {
                registry.broadcast(id, list);
            }
        }
        ClientMsg::Upvote { question } => {
            if let Some(list) = registry.upvote(id, who, question) {
                registry.broadcast(id, list);
            }
        }
        ClientMsg::Answered { question } => {
            if let Some(list) = registry.mark_answered(id, role, question) {
                registry.broadcast(id, list);
            }
        }
        // Anyone in the room may vote, the presenter included.
        ClientMsg::Answer { slide, option } => {
            if let Some(tally) = registry.answer(id, slide, who, option) {
                registry.broadcast(id, tally);
            }
        }
    }
}

/// Everything a socket needs to be correct again after missing messages.
async fn resync<S>(sink: &mut S, registry: &Registry, id: &str, is_owner: bool) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    if let Some(snapshot) = registry.snapshot(id) {
        send(sink, &snapshot, is_owner).await?;
    }
    if let Some(questions) = registry.questions(id) {
        send(sink, &questions, is_owner).await?;
    }
    if let Some(scores) = registry.scores(id) {
        send(sink, &scores, is_owner).await?;
    }
    for reveal in registry.reveals(id) {
        send(sink, &reveal, is_owner).await?;
    }
    if is_owner {
        for tally in registry.tallies(id) {
            send(sink, &tally, is_owner).await?;
        }
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
