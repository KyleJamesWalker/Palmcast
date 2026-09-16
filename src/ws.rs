use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::session::{Registry, Session};
use crate::wire::{ClientMsg, Frame, ServerMsg};

pub struct Join {
    pub id: String,
    pub token: Option<String>,
    /// Anonymous, browser generated, and only ever used to keep one voter from
    /// filling the tally. It is not an identity and not a defense against a
    /// determined cheat.
    pub who: String,
}

/// How long a socket that offered no token in its URL is given to send one.
/// Only a client that never sends the frame waits this out; every page sends it
/// the moment the socket opens.
const AUTH_WAIT: Duration = Duration::from_secs(5);

/// How often a socket is pinged, and how long it may say nothing at all.
///
/// A phone that drops off the network leaves a half open connection that TCP
/// will not notice for minutes, which inflates the viewer count and leaves the
/// phone showing `live` over a stale deck. A test builds these short.
#[derive(Clone, Copy)]
pub struct Heartbeat {
    pub beat: Duration,
    pub idle: Duration,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            beat: Duration::from_secs(25),
            idle: Duration::from_secs(60),
        }
    }
}

pub async fn serve(socket: WebSocket, registry: Registry, join: Join, beat: Heartbeat) {
    let Join { id, token, who } = join;
    let Some(mut rx) = registry.subscribe(&id) else {
        return;
    };
    let (mut sink, mut stream) = socket.split();

    // Browsers cannot set a header on a WebSocket, so a presenter's token
    // arrives as the first frame. Waiting for it means a presenter socket is
    // never hydrated as a viewer and corrected a moment later.
    let mut early = None;
    let token = match token {
        Some(token) => Some(token),
        None => match tokio::time::timeout(AUTH_WAIT, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::Auth { token }) => Some(token).filter(|t| !t.is_empty()),
                    // Not the auth frame, so this socket is an audience one.
                    // Hold what it sent rather than dropping it on the floor.
                    Ok(other) => {
                        early = Some(other);
                        None
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        },
    };

    // Notes, tallies and the deck source follow the ability to edit or drive,
    // so a co-host sees what they need to write the next question and a speaker
    // handed the controls sees their own notes.
    let mut is_staff = staff_now(&registry, &id, token.as_deref());

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
    // A room that will not take this phone says why, then closes. A viewer who
    // silently saw nothing would look like a broken app rather than a full one.
    match registry.with_mut(&id, |s| s.join(&who)) {
        Some(Ok(_)) => {}
        Some(Err(reason)) => {
            let _ = send(&mut sink, &ServerMsg::Refused { reason }, is_staff).await;
            return;
        }
        None => return,
    }
    if let Some(msg) = early.take() {
        handle(&registry, &id, token.as_deref(), &who, msg);
    }

    let mut ticker = tokio::time::interval(beat.beat);
    // The first tick is immediate, and a ping before the room has said anything
    // is noise.
    ticker.tick().await;
    let mut heard = tokio::time::Instant::now();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Nothing at all, not even a pong, for the whole window. The
                // far end is gone whatever TCP still believes.
                if heard.elapsed() >= beat.idle {
                    break;
                }
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
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
                            if registry.banned(&id, &who) {
                                let refused = ServerMsg::Refused {
                                    reason: crate::wire::Refusal::Removed,
                                };
                                let _ = send(&mut sink, &refused, is_staff).await;
                                break;
                            }
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
                // Any frame is proof of life, a pong as much as a vote.
                heard = tokio::time::Instant::now();
                let Message::Text(text) = frame else { continue };
                let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else {
                    continue;
                };
                // Answered here rather than in `handle`, because the reply goes
                // to this socket and nowhere else.
                if matches!(msg, ClientMsg::Ping) {
                    if send(&mut sink, &ServerMsg::Pong, is_staff).await.is_err() {
                        break;
                    }
                    continue;
                }
                if matches!(msg, ClientMsg::Resync) {
                    if resync(&mut sink, &registry, &id, is_staff).await.is_err() {
                        break;
                    }
                    continue;
                }
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
    token.is_some_and(|t| registry.staff(id, t))
}

/// Every branch re-checks the token inside the registry, so a forged frame from
/// a viewer changes nothing. The role is read at the moment the frame lands,
/// never captured when the socket opened: a speaker handed the controls must
/// start driving without reconnecting, and one handed them back must stop.
fn handle(registry: &Registry, id: &str, token: Option<&str>, who: &str, msg: ClientMsg) {
    let token = token.unwrap_or("");
    match msg {
        // Read once when the socket opened. A later one changes nothing, so a
        // viewer cannot talk its way into a presenter's socket.
        ClientMsg::Auth { .. } => {}
        // Answered on the socket it arrived on, before this is reached.
        ClientMsg::Ping | ClientMsg::Resync => {}
        ClientMsg::Goto { index, step } => {
            registry.with_mut(id, |s| s.goto(token, index, step));
        }
        ClientMsg::Qr { on } => {
            registry.with_mut(id, |s| s.show_qr(token, on));
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
        ClientMsg::Approval { on } => {
            registry.with_mut(id, |s| s.set_approval(s.role_of(token), on));
        }
        ClientMsg::Accept { talk } => {
            registry.with_mut(id, |s| s.accept(s.role_of(token), talk));
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
        ClientMsg::Lock { on } => {
            registry.with_mut(id, |s| s.set_lock(s.role_of(token), on));
        }
        ClientMsg::Moderate { on } => {
            registry.with_mut(id, |s| s.set_moderation(s.role_of(token), on));
        }
        ClientMsg::Approve { question } => {
            registry.with_mut(id, |s| s.approve(s.role_of(token), question));
        }
        ClientMsg::Dismiss { question } => {
            registry.with_mut(id, |s| s.dismiss(s.role_of(token), question));
        }
        ClientMsg::Kick { who: target } => {
            registry.with_mut(id, |s| s.kick(s.role_of(token), &target, who));
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
        ClientMsg::Respond { slide, text, value } => {
            registry.with_mut(id, |s| s.respond(slide, who, &text, value));
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
