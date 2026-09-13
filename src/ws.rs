use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};

use crate::session::Registry;
use crate::wire::{ClientMsg, ServerMsg};

pub async fn serve(socket: WebSocket, registry: Registry, id: String, token: Option<String>) {
    let is_owner = token.as_deref().is_some_and(|t| registry.owns(&id, t));
    let Some(mut rx) = registry.subscribe(&id) else {
        return;
    };
    let (mut sink, mut stream) = socket.split();

    if let Some(snapshot) = registry.snapshot(&id) {
        let opening = if is_owner {
            snapshot
        } else {
            snapshot.redacted()
        };
        if send(&mut sink, &opening, is_owner).await.is_err() {
            return;
        }
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
                let Ok(ClientMsg::Goto { index }) = serde_json::from_str(&text) else {
                    continue;
                };
                // goto re-checks the token, so a viewer forging a frame changes nothing.
                if let Some(token) = token.as_deref()
                    && let Some(moved) = registry.goto(&id, token, index)
                {
                    registry.broadcast(&id, moved);
                }
            }
        }
    }

    if let Some(count) = registry.leave(&id) {
        registry.broadcast(&id, count);
    }
}

async fn send<S>(sink: &mut S, msg: &ServerMsg, is_owner: bool) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let payload = if is_owner {
        msg.clone()
    } else {
        msg.redacted()
    };
    let text = serde_json::to_string(&payload).map_err(|_| ())?;
    sink.send(Message::Text(text.into())).await.map_err(|_| ())
}
