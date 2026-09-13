use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};

use crate::session::Registry;
use crate::wire::{ClientMsg, ServerMsg};

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
    let is_owner = token.as_deref().is_some_and(|t| registry.owns(&id, t));
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
    if let Some(count) = registry.join(&id) {
        registry.broadcast(&id, count);
    }

    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                let Ok(msg) = outgoing else { break };
                if send(&mut sink, &msg, is_owner).await.is_err() {
                    break;
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

    if let Some(count) = registry.leave(&id) {
        registry.broadcast(&id, count);
    }
}

/// Every branch re-checks the token inside the registry, so a forged frame from
/// a viewer changes nothing.
fn handle(registry: &Registry, id: &str, token: Option<&str>, who: &str, msg: ClientMsg) {
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
            if let Some(token) = token
                && let Some(list) = registry.mark_answered(id, token, question)
            {
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
