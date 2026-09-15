use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use palmcast::routes;
use palmcast::session::Registry;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

async fn spawn() -> String {
    let registry = Registry::new(Duration::from_secs(3600));
    serve(routes::router(registry)).await
}

async fn spawn_with_deck(markdown: &str) -> String {
    serve(routes::router_with(routes::App {
        registry: Registry::new(Duration::from_secs(3600)),
        public_url: None,
        starter: Some(markdown.to_string()),
        uploads: false,
        styles: Default::default(),
    }))
    .await
}

/// An instance that keeps pictures, which is not the default.
async fn spawn_with_uploads() -> String {
    serve(routes::router_with(routes::App {
        registry: Registry::new(Duration::from_secs(3600)),
        public_url: None,
        starter: None,
        uploads: true,
        styles: Default::default(),
    }))
    .await
}

async fn serve(router: axum::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("127.0.0.1:{}", addr.port())
}

async fn create(host: &str, markdown: &str) -> (String, String) {
    let body = serde_json::json!({ "markdown": markdown });
    let res = reqwest::Client::new()
        .post(format!("http://{host}/api/sessions"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let json: Value = res.json().await.unwrap();
    (
        json["id"].as_str().unwrap().to_string(),
        json["token"].as_str().unwrap().to_string(),
    )
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn open(host: &str, id: &str, token: Option<&str>) -> Socket {
    open_as(host, id, token, "anon").await
}

async fn open_as(host: &str, id: &str, token: Option<&str>, who: &str) -> Socket {
    let query = match token {
        Some(t) => format!("?who={who}&token={t}"),
        None => format!("?who={who}"),
    };
    let (socket, _) = connect_async(format!("ws://{host}/s/{id}/ws{query}"))
        .await
        .unwrap();
    socket
}

async fn next_json(socket: &mut Socket) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("socket closed")
            .expect("socket error");
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

const DECK: &str = "# One\n\n???\nthe secret note\n\n---\n\n# Two\n\n---\n\n# Three";

#[tokio::test]
async fn the_audience_never_receives_speaker_notes() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut audience = open(&host, &id, None).await;
    let opening = next_json(&mut audience).await;

    assert_eq!(opening["type"], "deck");
    let raw = opening.to_string();
    assert!(
        !raw.contains("the secret note"),
        "speaker notes leaked to the audience: {raw}"
    );
    for slide in opening["slides"].as_array().unwrap() {
        assert_eq!(slide["notes"], "");
    }
}

#[tokio::test]
async fn the_presenter_does_receive_speaker_notes() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut presenter = open(&host, &id, Some(&token)).await;
    let opening = next_json(&mut presenter).await;

    assert_eq!(opening["type"], "deck");
    assert_eq!(opening["slides"][0]["notes"], "the secret note");
}

#[tokio::test]
async fn a_wrong_token_is_treated_as_audience() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut imposter = open(&host, &id, Some("not-the-real-token")).await;
    let opening = next_json(&mut imposter).await;
    assert_eq!(opening["slides"][0]["notes"], "");
}

#[tokio::test]
async fn the_presenter_moves_every_viewer() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut audience = open(&host, &id, None).await;
    let _ = next_json(&mut audience).await;

    let mut presenter = open(&host, &id, Some(&token)).await;
    let _ = next_json(&mut presenter).await;

    presenter
        .send(Message::Text(r#"{"type":"goto","index":2}"#.into()))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut audience).await;
        if msg["type"] == "move" {
            assert_eq!(msg["current"], 2);
            break;
        }
    }
}

#[tokio::test]
async fn a_viewer_cannot_move_the_deck() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut heckler = open(&host, &id, None).await;
    let _ = next_json(&mut heckler).await;
    heckler
        .send(Message::Text(r#"{"type":"goto","index":2}"#.into()))
        .await
        .unwrap();

    // Give the server a moment to do the wrong thing, then prove it did not.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut presenter = open(&host, &id, Some(&token)).await;
    let opening = next_json(&mut presenter).await;
    assert_eq!(opening["current"], 0, "a viewer moved the deck");
}

#[tokio::test]
async fn an_out_of_range_index_is_refused() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut presenter = open(&host, &id, Some(&token)).await;
    let _ = next_json(&mut presenter).await;
    presenter
        .send(Message::Text(r#"{"type":"goto","index":99}"#.into()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut checker = open(&host, &id, Some(&token)).await;
    let opening = next_json(&mut checker).await;
    assert_eq!(opening["current"], 0);
}

const QUIZ: &str = "# Year Rust 1.0 shipped?\n\n- [ ] 2012\n- [x] 2015\n- [ ] 2018";

#[tokio::test]
async fn the_audience_never_receives_the_right_answer() {
    let host = spawn().await;
    let (id, _token) = create(&host, QUIZ).await;

    let mut audience = open(&host, &id, None).await;
    let opening = next_json(&mut audience).await;

    let question = &opening["slides"][0]["question"];
    assert_eq!(question["options"][1], "2015");
    assert_eq!(
        question["correct"].as_array().unwrap().len(),
        0,
        "the answer leaked to the audience: {question}"
    );
}

#[tokio::test]
async fn the_presenter_does_receive_the_right_answer() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut presenter = open(&host, &id, Some(&token)).await;
    let opening = next_json(&mut presenter).await;

    assert_eq!(opening["slides"][0]["question"]["correct"][0], 1);
}

#[tokio::test]
async fn a_vote_reaches_the_presenter_as_a_tally() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut presenter).await;
        if msg["type"] == "tally" {
            assert_eq!(msg["counts"][1], 1);
            assert_eq!(msg["total"], 1);
            break;
        }
    }
}

#[tokio::test]
async fn the_audience_never_sees_the_running_tally() {
    let host = spawn().await;
    let (id, _token) = create(&host, QUIZ).await;

    let mut watcher = open_as(&host, &id, None, "watcher").await;
    let _ = next_json(&mut watcher).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;

    // Anything queued for the watcher must not be a tally.
    let pending = tokio::time::timeout(Duration::from_millis(250), watcher.next()).await;
    if let Ok(Some(Ok(Message::Text(text)))) = pending {
        let msg: Value = serde_json::from_str(&text).unwrap();
        assert_ne!(msg["type"], "tally", "the tally leaked to the audience");
    }
}

#[tokio::test]
async fn one_voter_cannot_stuff_the_tally() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    for option in [0, 1, 2, 1] {
        voter
            .send(Message::Text(
                format!(r#"{{"type":"answer","slide":0,"options":[{option}]}}"#).into(),
            ))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(250)).await;

    let mut last = None;
    while let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(250), presenter.next()).await
    {
        let msg: Value = serde_json::from_str(&text).unwrap();
        if msg["type"] == "tally" {
            last = Some(msg);
        }
    }
    let tally = last.expect("expected at least one tally");
    assert_eq!(tally["total"], 1, "one voter produced more than one vote");
    assert_eq!(tally["counts"][1], 1);
}

#[tokio::test]
async fn reveal_sends_the_answer_to_the_whole_room() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut audience = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut audience).await;

    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;
    presenter
        .send(Message::Text(r#"{"type":"reveal","slide":0}"#.into()))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut audience).await;
        if msg["type"] == "reveal" {
            assert_eq!(msg["correct"][0], 1);
            break;
        }
    }
}

#[tokio::test]
async fn a_viewer_cannot_reveal_the_answer() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut heckler = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut heckler).await;
    heckler
        .send(Message::Text(r#"{"type":"reveal","slide":0}"#.into()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;

    // A vote still lands, which it could not do if the slide had been revealed.
    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;
    heckler
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[0]}"#.into(),
        ))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut presenter).await;
        if msg["type"] == "tally" {
            assert_eq!(msg["total"], 1);
            break;
        }
    }
}

#[tokio::test]
async fn a_vote_after_the_reveal_is_refused() {
    let host = spawn().await;
    let (id, token) = create(&host, QUIZ).await;

    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;
    presenter
        .send(Message::Text(r#"{"type":"reveal","slide":0}"#.into()))
        .await
        .unwrap();
    loop {
        if next_json(&mut presenter).await["type"] == "reveal" {
            break;
        }
    }

    let mut latecomer = open_as(&host, &id, None, "late").await;
    let _ = next_json(&mut latecomer).await;
    latecomer
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[0]}"#.into(),
        ))
        .await
        .unwrap();

    let pending = tokio::time::timeout(Duration::from_millis(400), presenter.next()).await;
    if let Ok(Some(Ok(Message::Text(text)))) = pending {
        let msg: Value = serde_json::from_str(&text).unwrap();
        assert_ne!(msg["type"], "tally", "a vote landed after the reveal");
    }
}

#[tokio::test]
async fn a_reaction_reaches_the_whole_room() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut watcher = open_as(&host, &id, None, "watcher").await;
    let _ = next_json(&mut watcher).await;

    let mut reactor = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut reactor).await;
    reactor
        .send(Message::Text(r#"{"type":"react","kind":"clap"}"#.into()))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut watcher).await;
        if msg["type"] == "react" {
            assert_eq!(msg["kind"], "clap");
            break;
        }
    }
}

#[tokio::test]
async fn a_reaction_is_rate_limited_per_viewer() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut watcher = open_as(&host, &id, None, "watcher").await;
    let _ = next_json(&mut watcher).await;

    let mut spammer = open_as(&host, &id, None, "spam").await;
    let _ = next_json(&mut spammer).await;
    for _ in 0..10 {
        spammer
            .send(Message::Text(r#"{"type":"react","kind":"laugh"}"#.into()))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut seen = 0;
    while let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(250), watcher.next()).await
    {
        let msg: Value = serde_json::from_str(&text).unwrap();
        if msg["type"] == "react" {
            seen += 1;
        }
    }
    assert_eq!(
        seen, 1,
        "ten taps inside the gap produced {seen} broadcasts"
    );
}

#[tokio::test]
async fn an_unknown_reaction_is_dropped() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut watcher = open_as(&host, &id, None, "watcher").await;
    let _ = next_json(&mut watcher).await;

    let mut sneak = open_as(&host, &id, None, "sneak").await;
    let _ = next_json(&mut sneak).await;
    sneak
        .send(Message::Text(
            r#"{"type":"react","kind":"<img src=x onerror=alert(1)>"}"#.into(),
        ))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;

    let pending = tokio::time::timeout(Duration::from_millis(250), watcher.next()).await;
    if let Ok(Some(Ok(Message::Text(text)))) = pending {
        let msg: Value = serde_json::from_str(&text).unwrap();
        assert_ne!(msg["type"], "react", "an unknown reaction was broadcast");
    }
}

async fn next_questions(socket: &mut Socket) -> Value {
    loop {
        let msg = next_json(socket).await;
        if msg["type"] == "questions" {
            return msg;
        }
    }
}

#[tokio::test]
async fn a_question_reaches_the_room_with_the_asker_s_own_vote() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut watcher = open_as(&host, &id, None, "watcher").await;
    let _ = next_json(&mut watcher).await;
    let _ = next_questions(&mut watcher).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    asker
        .send(Message::Text(
            r#"{"type":"ask","text":"Why not Go?"}"#.into(),
        ))
        .await
        .unwrap();

    let list = next_questions(&mut watcher).await;
    assert_eq!(list["items"][0]["text"], "Why not Go?");
    assert_eq!(list["items"][0]["votes"], 1);
    assert_eq!(list["items"][0]["answered"], false);
}

#[tokio::test]
async fn one_viewer_cannot_upvote_the_same_question_twice() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    let _ = next_questions(&mut asker).await;
    asker
        .send(Message::Text(
            r#"{"type":"ask","text":"Question one"}"#.into(),
        ))
        .await
        .unwrap();
    let list = next_questions(&mut asker).await;
    let question = list["items"][0]["id"].as_u64().unwrap();

    let mut voter = open_as(&host, &id, None, "alex").await;
    let _ = next_json(&mut voter).await;
    let _ = next_questions(&mut voter).await;
    for _ in 0..5 {
        voter
            .send(Message::Text(
                format!(r#"{{"type":"upvote","question":{question}}}"#).into(),
            ))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut latest = None;
    while let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(250), asker.next()).await
    {
        let msg: Value = serde_json::from_str(&text).unwrap();
        if msg["type"] == "questions" {
            latest = Some(msg);
        }
    }
    let list = latest.expect("expected a question list");
    assert_eq!(list["items"][0]["votes"], 2, "a repeat tap counted twice");
}

#[tokio::test]
async fn an_over_long_question_is_refused() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    let _ = next_questions(&mut asker).await;

    let essay = "x".repeat(281);
    asker
        .send(Message::Text(
            serde_json::json!({"type": "ask", "text": essay})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let pending = tokio::time::timeout(Duration::from_millis(400), asker.next()).await;
    if let Ok(Some(Ok(Message::Text(text)))) = pending {
        let msg: Value = serde_json::from_str(&text).unwrap();
        assert_ne!(msg["type"], "questions", "an over long question landed");
    }
}

#[tokio::test]
async fn asking_is_rate_limited_per_viewer() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    let _ = next_questions(&mut asker).await;
    for n in 0..5 {
        asker
            .send(Message::Text(
                serde_json::json!({"type": "ask", "text": format!("question {n}")})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut latest = None;
    while let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(250), asker.next()).await
    {
        let msg: Value = serde_json::from_str(&text).unwrap();
        if msg["type"] == "questions" {
            latest = Some(msg);
        }
    }
    let list = latest.expect("expected a question list");
    assert_eq!(
        list["items"].as_array().unwrap().len(),
        1,
        "five rapid asks were not throttled to one"
    );
}

#[tokio::test]
async fn a_viewer_cannot_mark_a_question_answered() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    let _ = next_questions(&mut asker).await;
    asker
        .send(Message::Text(r#"{"type":"ask","text":"Answer me"}"#.into()))
        .await
        .unwrap();
    let list = next_questions(&mut asker).await;
    let question = list["items"][0]["id"].as_u64().unwrap();

    asker
        .send(Message::Text(
            format!(r#"{{"type":"answered","question":{question}}}"#).into(),
        ))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut checker = open_as(&host, &id, None, "check").await;
    let _ = next_json(&mut checker).await;
    let list = next_questions(&mut checker).await;
    assert_eq!(
        list["items"][0]["answered"], false,
        "a viewer closed a question"
    );
}

#[tokio::test]
async fn the_presenter_can_mark_a_question_answered() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut asker = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut asker).await;
    let _ = next_questions(&mut asker).await;
    asker
        .send(Message::Text(r#"{"type":"ask","text":"Answer me"}"#.into()))
        .await
        .unwrap();
    let list = next_questions(&mut asker).await;
    let question = list["items"][0]["id"].as_u64().unwrap();

    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;
    presenter
        .send(Message::Text(
            format!(r#"{{"type":"answered","question":{question}}}"#).into(),
        ))
        .await
        .unwrap();

    loop {
        let list = next_questions(&mut asker).await;
        if list["items"][0]["answered"] == true {
            break;
        }
    }
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    for path in ["/", &format!("/s/{id}"), "/base.css"] {
        let res = reqwest::get(format!("http://{host}{path}")).await.unwrap();
        let headers = res.headers();
        let csp = headers
            .get("content-security-policy")
            .expect("no content security policy")
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'self'"), "{path}: {csp}");
        assert!(csp.contains("object-src 'none'"), "{path}: {csp}");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    }
}

#[tokio::test]
async fn an_oversized_deck_is_refused() {
    let host = spawn().await;
    let body = serde_json::json!({ "markdown": "x".repeat(300 * 1024) });
    let res = reqwest::Client::new()
        .post(format!("http://{host}/api/sessions"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_client_error(),
        "a 300KB deck got {}",
        res.status()
    );
}

#[tokio::test]
async fn a_lagging_socket_is_resynced_not_dropped() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;

    let mut slow = open_as(&host, &id, None, "slow").await;
    let _ = next_json(&mut slow).await;

    // Outrun the 64 slot broadcast channel without reading a single frame.
    let mut presenter = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut presenter).await;
    for n in 0..200 {
        presenter
            .send(Message::Text(
                format!(r#"{{"type":"goto","index":{}}}"#, n % 3).into(),
            ))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    // The socket must still be usable, and must be able to say where it is.
    let mut saw_state = false;
    for _ in 0..400 {
        match tokio::time::timeout(Duration::from_millis(500), slow.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                if msg["type"] == "deck" || msg["type"] == "move" {
                    saw_state = true;
                    break;
                }
            }
            Ok(Some(Ok(_))) => continue,
            _ => break,
        }
    }
    assert!(
        saw_state,
        "a lagging socket was dropped instead of resynced"
    );
}

#[tokio::test]
async fn health_reports_counts_and_nothing_else() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;
    let mut viewer = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut viewer).await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let res = reqwest::get(format!("http://{host}/healthz"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["sessions"], 1);
    assert_eq!(body["viewers"], 1);

    // Nothing identifying a room may appear in a probe response.
    let raw = body.to_string();
    assert!(!raw.contains(&id), "health leaked a session id: {raw}");
}

const TWO_QUIZ: &str = "# One?\n\n- [ ] a\n- [x] b\n\n---\n\n# Two?\n\n- [x] c\n- [ ] d";

async fn next_scores(socket: &mut Socket) -> Value {
    loop {
        let msg = next_json(socket).await;
        if msg["type"] == "scores" {
            return msg;
        }
    }
}

#[tokio::test]
async fn only_named_people_reach_the_board() {
    let host = spawn().await;
    let (id, _token) = create(&host, TWO_QUIZ).await;

    let mut anon = open_as(&host, &id, None, "anon").await;
    let _ = next_json(&mut anon).await;

    let mut named = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut named).await;
    // A socket is sent the board on connect, so skip that one and read the
    // board the naming produces.
    let opening = next_scores(&mut named).await;
    assert_eq!(
        opening["items"].as_array().unwrap().len(),
        0,
        "an unnamed viewer was already on the board"
    );

    named
        .send(Message::Text(r#"{"type":"set_name","name":"Sam"}"#.into()))
        .await
        .unwrap();

    let board = next_scores(&mut named).await;
    let items = board["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "expected only the named viewer: {board}");
    assert_eq!(items[0]["name"], "Sam");
    assert_eq!(items[0]["score"], 0);
}

#[tokio::test]
async fn a_right_answer_scores_when_the_presenter_reveals() {
    let host = spawn().await;
    let (id, token) = create(&host, TWO_QUIZ).await;

    let mut player = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut player).await;
    player
        .send(Message::Text(r#"{"type":"set_name","name":"Sam"}"#.into()))
        .await
        .unwrap();
    let _ = next_scores(&mut player).await;

    // Right on slide 0, wrong on slide 1.
    player
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();
    player
        .send(Message::Text(
            r#"{"type":"answer","slide":1,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut mc).await;
    mc.send(Message::Text(r#"{"type":"reveal","slide":0}"#.into()))
        .await
        .unwrap();

    loop {
        let board = next_scores(&mut player).await;
        if board["items"][0]["score"] == 1 {
            break;
        }
    }

    mc.send(Message::Text(r#"{"type":"reveal","slide":1}"#.into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut checker = open_as(&host, &id, None, "check").await;
    let _ = next_json(&mut checker).await;
    let board = next_scores(&mut checker).await;
    assert_eq!(
        board["items"][0]["score"], 1,
        "a wrong answer scored: {board}"
    );
}

#[tokio::test]
async fn an_unrevealed_question_scores_nobody() {
    let host = spawn().await;
    let (id, _token) = create(&host, TWO_QUIZ).await;

    let mut player = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut player).await;
    player
        .send(Message::Text(r#"{"type":"set_name","name":"Sam"}"#.into()))
        .await
        .unwrap();
    let _ = next_scores(&mut player).await;
    player
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut checker = open_as(&host, &id, None, "check").await;
    let _ = next_json(&mut checker).await;
    let board = next_scores(&mut checker).await;
    assert_eq!(
        board["items"][0]["score"], 0,
        "scoring leaked before the reveal: {board}"
    );
}

#[tokio::test]
async fn an_over_long_name_is_refused() {
    let host = spawn().await;
    let (id, _token) = create(&host, TWO_QUIZ).await;

    let mut player = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut player).await;
    let _ = next_scores(&mut player).await;
    player
        .send(Message::Text(
            serde_json::json!({"type": "set_name", "name": "n".repeat(25)})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let pending = tokio::time::timeout(Duration::from_millis(400), player.next()).await;
    if let Ok(Some(Ok(Message::Text(text)))) = pending {
        let msg: Value = serde_json::from_str(&text).unwrap();
        assert_ne!(msg["type"], "scores", "an over long name landed");
    }
}

async fn put_deck(host: &str, id: &str, token: &str, markdown: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .put(format!("http://{host}/api/sessions/{id}?token={token}"))
        .json(&serde_json::json!({ "markdown": markdown }))
        .send()
        .await
        .unwrap()
        .status()
}

const EDIT_DECK: &str = "# Intro\n\n---\n\n# Q one\n\n- [ ] a\n- [x] b";

#[tokio::test]
async fn editing_a_typo_keeps_the_votes_on_an_untouched_question() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":1,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Fix the first slide only.
    let status = put_deck(
        &host,
        &id,
        &token,
        "# Introduction\n\n---\n\n# Q one\n\n- [ ] a\n- [x] b",
    )
    .await;
    assert_eq!(status, 204);

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut mc).await;
    mc.send(Message::Text(r#"{"type":"reveal","slide":1}"#.into()))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut mc).await;
        if msg["type"] == "reveal" {
            assert_eq!(msg["total"], 1, "an unrelated edit dropped the vote");
            break;
        }
    }
}

#[tokio::test]
async fn changing_the_options_drops_that_question_s_votes() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":1,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Same slide, different options.
    let status = put_deck(
        &host,
        &id,
        &token,
        "# Intro\n\n---\n\n# Q one\n\n- [ ] x\n- [x] y\n- [ ] z",
    )
    .await;
    assert_eq!(status, 204);

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut mc).await;
    mc.send(Message::Text(r#"{"type":"reveal","slide":1}"#.into()))
        .await
        .unwrap();

    loop {
        let msg = next_json(&mut mc).await;
        if msg["type"] == "reveal" {
            assert_eq!(msg["total"], 0, "a vote survived an options change");
            break;
        }
    }
}

#[tokio::test]
async fn a_viewer_cannot_edit_the_deck() {
    let host = spawn().await;
    let (id, _token) = create(&host, EDIT_DECK).await;
    let status = put_deck(&host, &id, "not-the-token", "# Hijacked").await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn an_edit_reaches_every_viewer() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;

    let mut audience = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut audience).await;

    put_deck(&host, &id, &token, "# Rewritten\n\n---\n\n# Also new").await;

    loop {
        let msg = next_json(&mut audience).await;
        if msg["type"] == "deck" {
            let raw = msg.to_string();
            if raw.contains("Rewritten") {
                assert!(msg["rev"].as_u64().unwrap() > 1, "rev did not advance");
                break;
            }
        }
    }
}

#[tokio::test]
async fn an_asset_revalidates_rather_than_caching_blind() {
    let host = spawn().await;
    let client = reqwest::Client::new();

    let first = client
        .get(format!("http://{host}/present.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(
        first.headers().get("cache-control").unwrap(),
        "no-cache",
        "an asset without no-cache can outlive the binary that served it"
    );
    let tag = first
        .headers()
        .get("etag")
        .expect("no etag")
        .to_str()
        .unwrap()
        .to_string();

    let second = client
        .get(format!("http://{host}/present.js"))
        .header("if-none-match", &tag)
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 304, "a matching etag should not resend");

    let changed = client
        .get(format!("http://{host}/present.js"))
        .header("if-none-match", "\"deadbeefdeadbeef\"")
        .send()
        .await
        .unwrap();
    assert_eq!(changed.status(), 200, "a stale etag must resend");
}

#[tokio::test]
async fn different_assets_carry_different_tags() {
    let host = spawn().await;
    let client = reqwest::Client::new();
    let mut tags = Vec::new();
    for path in ["/present.js", "/watch.js", "/base.css"] {
        let res = client
            .get(format!("http://{host}{path}"))
            .send()
            .await
            .unwrap();
        tags.push(
            res.headers()
                .get("etag")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
        );
    }
    tags.sort();
    tags.dedup();
    assert_eq!(tags.len(), 3, "two assets shared an etag");
}

#[tokio::test]
async fn a_client_can_tell_a_missing_room_from_a_missing_server() {
    let host = spawn().await;
    let (id, _token) = create(&host, DECK).await;

    let alive = reqwest::get(format!("http://{host}/api/sessions/{id}"))
        .await
        .unwrap();
    assert_eq!(alive.status(), 204);

    let gone = reqwest::get(format!("http://{host}/api/sessions/nosuchroom"))
        .await
        .unwrap();
    assert_eq!(gone.status(), 404);
}

#[tokio::test]
async fn the_existence_probe_leaks_nothing_about_the_room() {
    let host = spawn().await;
    let (id, token) = create(&host, "# Secret deck\n\n???\nprivate note").await;

    let res = reqwest::get(format!("http://{host}/api/sessions/{id}"))
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    assert!(body.is_empty(), "the probe returned a body: {body}");
    assert!(!body.contains(&token));
}

#[tokio::test]
async fn a_rewritten_asset_tags_the_bytes_it_actually_sends() {
    let host = spawn().await;
    let client = reqwest::Client::new();

    // present.js is rewritten to carry the build id, so its tag must depend on
    // the build and not only on the file.
    let res = client
        .get(format!("http://{host}/present.js"))
        .send()
        .await
        .unwrap();
    let tag = res
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let body = res.text().await.unwrap();

    let build = body
        .split("?v=")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .expect("no build id in the rewritten body")
        .to_string();
    assert!(
        tag.contains(&build),
        "etag {tag} does not cover the build id {build} it served"
    );

    // A stylesheet is not rewritten, so its tag stays the plain digest.
    let css = client
        .get(format!("http://{host}/base.css"))
        .send()
        .await
        .unwrap();
    let css_tag = css.headers().get("etag").unwrap().to_str().unwrap();
    assert!(
        !css_tag.contains(&build),
        "a plain asset carried the build id"
    );
}

#[tokio::test]
async fn a_presenter_joining_mid_round_is_told_the_tally() {
    let host = spawn().await;
    let (id, token) = create(&host, TWO_QUIZ).await;

    // The room votes before anyone opens the console.
    for who in ["sam", "alex", "robin"] {
        let mut voter = open_as(&host, &id, None, who).await;
        let _ = next_json(&mut voter).await;
        voter
            .send(Message::Text(
                r#"{"type":"answer","slide":0,"options":[0]}"#.into(),
            ))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        std::mem::forget(voter);
    }

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;

    let mut saw = None;
    for _ in 0..12 {
        let msg = next_json(&mut mc).await;
        if msg["type"] == "tally" && msg["slide"] == 0 {
            saw = Some(msg);
            break;
        }
    }
    let tally = saw.expect("the console was never told how the room had voted");
    assert_eq!(tally["total"], 3);
    assert_eq!(tally["counts"][0], 3);
}

#[tokio::test]
async fn a_viewer_joining_mid_round_is_not_told_the_tally() {
    let host = spawn().await;
    let (id, _token) = create(&host, TWO_QUIZ).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[0]}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut latecomer = open_as(&host, &id, None, "late").await;
    for _ in 0..6 {
        match tokio::time::timeout(Duration::from_millis(400), latecomer.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                assert_ne!(msg["type"], "tally", "the room saw the split forming");
            }
            _ => break,
        }
    }
}

#[tokio::test]
async fn a_viewer_arriving_after_a_reveal_is_shown_the_answer() {
    let host = spawn().await;
    let (id, token) = create(&host, TWO_QUIZ).await;

    let mut early = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut early).await;
    early
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;
    let _ = next_json(&mut mc).await;
    mc.send(Message::Text(r#"{"type":"reveal","slide":0}"#.into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Somebody's phone dropped and came back, or they walked in late.
    let mut latecomer = open_as(&host, &id, None, "late").await;
    let mut seen = None;
    for _ in 0..12 {
        let msg = next_json(&mut latecomer).await;
        if msg["type"] == "reveal" && msg["slide"] == 0 {
            seen = Some(msg);
            break;
        }
    }
    let reveal = seen.expect("a viewer who arrived after the reveal never saw the answer");
    assert_eq!(reveal["correct"][0], 1);
    assert_eq!(reveal["total"], 1);
}

#[tokio::test]
async fn an_unrevealed_question_is_still_withheld_from_an_arriving_viewer() {
    let host = spawn().await;
    let (id, _token) = create(&host, TWO_QUIZ).await;

    let mut voter = open_as(&host, &id, None, "sam").await;
    let _ = next_json(&mut voter).await;
    voter
        .send(Message::Text(
            r#"{"type":"answer","slide":0,"options":[1]}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut latecomer = open_as(&host, &id, None, "late").await;
    let opening = next_json(&mut latecomer).await;
    assert_eq!(opening["type"], "deck");
    for slide in opening["slides"].as_array().unwrap() {
        assert_eq!(slide["question"]["correct"].as_array().unwrap().len(), 0);
    }
    for _ in 0..6 {
        match tokio::time::timeout(Duration::from_millis(350), latecomer.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                assert_ne!(msg["type"], "reveal", "an unrevealed answer was handed out");
            }
            _ => break,
        }
    }
}

async fn cohost_token(host: &str, id: &str, mc: &str) -> String {
    let res = reqwest::get(format!("http://{host}/api/sessions/{id}/cohost?token={mc}"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    res.text().await.unwrap()
}

#[tokio::test]
async fn only_the_mc_can_mint_a_cohost_link() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;

    let cohost = cohost_token(&host, &id, &token).await;
    assert!(!cohost.is_empty());
    assert_ne!(cohost, token, "the cohost link is just the mc token again");

    for wrong in ["", "not-a-token", cohost.as_str()] {
        let res = reqwest::get(format!(
            "http://{host}/api/sessions/{id}/cohost?token={wrong}"
        ))
        .await
        .unwrap();
        assert_eq!(res.status(), 403, "a cohost link was handed to {wrong:?}");
    }
}

#[tokio::test]
async fn a_cohost_can_edit_the_deck() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;
    let cohost = cohost_token(&host, &id, &token).await;

    let status = put_deck(&host, &id, &cohost, "# Written by the cohost").await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn a_cohost_cannot_drive_the_room() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;
    let cohost = cohost_token(&host, &id, &token).await;

    let mut second = open_as(&host, &id, Some(&cohost), "cohost").await;
    let _ = next_json(&mut second).await;
    second
        .send(Message::Text(r#"{"type":"goto","index":1}"#.into()))
        .await
        .unwrap();
    second
        .send(Message::Text(r#"{"type":"reveal","slide":1}"#.into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut mc = open_as(&host, &id, Some(&token), "mc").await;
    let opening = next_json(&mut mc).await;
    assert_eq!(opening["current"], 0, "a cohost moved the room");
    for slide in opening["slides"].as_array().unwrap() {
        // A reveal would have gone out to everyone, so none may have happened.
        assert!(slide["question"].is_null() || slide["question"]["correct"].is_array());
    }
}

#[tokio::test]
async fn a_cohost_sees_the_speaker_notes() {
    let host = spawn().await;
    let (id, token) = create(&host, DECK).await;
    let cohost = cohost_token(&host, &id, &token).await;

    let mut second = open_as(&host, &id, Some(&cohost), "cohost").await;
    let opening = next_json(&mut second).await;
    assert_eq!(opening["slides"][0]["notes"], "the secret note");
}

#[tokio::test]
async fn a_viewer_still_cannot_edit_or_read_the_source() {
    let host = spawn().await;
    let (id, _token) = create(&host, EDIT_DECK).await;

    assert_eq!(put_deck(&host, &id, "guessed", "# Hijacked").await, 403);
    let res = reqwest::get(format!(
        "http://{host}/api/sessions/{id}/markdown?token=guessed"
    ))
    .await
    .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn the_second_editor_to_save_is_told_rather_than_overwriting() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;
    let cohost = cohost_token(&host, &id, &token).await;
    let client = reqwest::Client::new();

    // Both opened the deck at revision 1.
    let first = client
        .put(format!(
            "http://{host}/api/sessions/{id}?token={token}&rev=1"
        ))
        .json(&serde_json::json!({ "markdown": "# The mc got there first" }))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 204);

    let second = client
        .put(format!(
            "http://{host}/api/sessions/{id}?token={cohost}&rev=1"
        ))
        .json(&serde_json::json!({ "markdown": "# The cohost would have clobbered it" }))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 409, "the second save overwrote the first");

    let kept = reqwest::get(format!(
        "http://{host}/api/sessions/{id}/markdown?token={token}"
    ))
    .await
    .unwrap()
    .text()
    .await
    .unwrap();
    assert!(kept.contains("mc got there first"), "kept: {kept}");
}

#[tokio::test]
async fn a_save_without_a_revision_still_works() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;
    assert_eq!(
        put_deck(&host, &id, &token, "# No revision given").await,
        204
    );
}

#[tokio::test]
async fn a_cohost_cannot_hand_out_further_cohost_links() {
    let host = spawn().await;
    let (id, token) = create(&host, EDIT_DECK).await;
    let cohost = cohost_token(&host, &id, &token).await;

    let res = reqwest::get(format!(
        "http://{host}/api/sessions/{id}/cohost?token={cohost}"
    ))
    .await
    .unwrap();
    assert_eq!(res.status(), 403, "a cohost minted another cohost");
}

#[tokio::test]
async fn a_cohost_token_survives_the_save_and_load_round_trip() {
    use palmcast::session::{Registry, Role};

    let before = Registry::new(Duration::from_secs(3600));
    let (id, mc) = before.create("# Deck").unwrap();
    let cohost = before.cohost_token(&id, &mc).expect("no cohost token");

    let after = Registry::new(Duration::from_secs(3600));
    after.import(before.export());

    assert_eq!(after.role(&id, &mc), Role::Mc);
    assert_eq!(after.role(&id, &cohost), Role::CoHost);
    assert_eq!(after.role(&id, "guessed"), Role::Viewer);
    assert_eq!(
        after.role(&id, ""),
        Role::Viewer,
        "an empty token authenticated"
    );
}

async fn post_json(host: &str, path: &str, body: Value) -> (u16, String) {
    let res = reqwest::Client::new()
        .post(format!("http://{host}{path}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.text().await.unwrap())
}

#[tokio::test]
async fn a_deck_link_survives_a_trip_through_the_api() {
    let host = spawn().await;
    let deck = "# Quiz night\n\n---\n\n- [x] yes\n- [ ] no\n\n???\nNotes travel too.\n";

    let (status, body) =
        post_json(&host, "/api/pack", serde_json::json!({ "markdown": deck })).await;
    assert_eq!(status, 200);
    let token = serde_json::from_str::<Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) =
        post_json(&host, "/api/unpack", serde_json::json!({ "token": token })).await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["markdown"]
            .as_str()
            .unwrap(),
        deck
    );
}

#[tokio::test]
async fn a_deck_from_a_link_starts_a_real_session() {
    let host = spawn().await;
    let deck = "# One\n\n---\n\n# Two\n";
    let (_, body) = post_json(&host, "/api/pack", serde_json::json!({ "markdown": deck })).await;
    let token = serde_json::from_str::<Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, body) = post_json(&host, "/api/unpack", serde_json::json!({ "token": token })).await;
    let markdown = serde_json::from_str::<Value>(&body).unwrap()["markdown"]
        .as_str()
        .unwrap()
        .to_string();

    let (id, presenter) = create(&host, &markdown).await;
    let mut socket = open(&host, &id, Some(&presenter)).await;
    let deck_msg = next_json(&mut socket).await;
    assert_eq!(deck_msg["slides"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_damaged_deck_link_is_refused_with_a_reason() {
    let host = spawn().await;
    let (status, body) = post_json(
        &host,
        "/api/unpack",
        serde_json::json!({ "token": "not a token" }),
    )
    .await;
    assert_eq!(status, 400);
    assert!(body.contains("not a Palmcast deck"), "got {body}");
}

#[tokio::test]
async fn a_deck_too_long_to_share_is_refused_by_the_api() {
    let host = spawn().await;
    let deck = "a".repeat(palmcast::share::MAX_SHARE_BYTES + 1);
    let (status, body) =
        post_json(&host, "/api/pack", serde_json::json!({ "markdown": deck })).await;
    assert_eq!(status, 413);
    assert!(body.contains("too long to share"), "got {body}");
}

/// A room accumulates things the audience contributed. A link that carried
/// those back would be a leak dressed up as a convenience.
#[tokio::test]
async fn a_deck_link_carries_the_slides_and_nothing_the_room_added() {
    let host = spawn().await;
    let deck = "# Quiz\n\n---\n\n- [x] yes\n- [ ] no\n";
    let (id, presenter) = create(&host, deck).await;

    let mut viewer = open_as(&host, &id, None, "guest").await;
    viewer
        .send(Message::Text(
            serde_json::json!({ "type": "ask", "text": "who is buying" })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    viewer
        .send(Message::Text(
            serde_json::json!({ "type": "answer", "slide": 1, "options": [0] })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let markdown = reqwest::get(format!(
        "http://{host}/api/sessions/{id}/markdown?token={presenter}"
    ))
    .await
    .unwrap()
    .text()
    .await
    .unwrap();

    let (_, body) = post_json(
        &host,
        "/api/pack",
        serde_json::json!({ "markdown": markdown }),
    )
    .await;
    let token = serde_json::from_str::<Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, body) = post_json(&host, "/api/unpack", serde_json::json!({ "token": token })).await;
    let shared = serde_json::from_str::<Value>(&body).unwrap()["markdown"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(shared, deck);
    assert!(!shared.contains("who is buying"), "a question rode along");
    assert!(!shared.contains("guest"), "a participant rode along");
}

/// The preview exists to show the author what the room will get. If the two
/// paths ever render differently, the preview is worse than not having one.
#[tokio::test]
async fn the_preview_renders_exactly_what_the_room_will_see() {
    let host = spawn().await;
    let deck = "# Hi <script>alert(1)</script>\n\n---\n\n```yaml\na: 1\n---\nb: 2\n```\n\n\
                ---\n\n## Pick\n\n- [x] one\n- [x] two\n- [ ] three\n\n???\nNotes.\n";

    let (_, body) = post_json(
        &host,
        "/api/preview",
        serde_json::json!({ "markdown": deck }),
    )
    .await;
    let previewed = serde_json::from_str::<Value>(&body).unwrap()["slides"].clone();

    let (id, presenter) = create(&host, deck).await;
    let mut socket = open(&host, &id, Some(&presenter)).await;
    let presented = next_json(&mut socket).await["slides"].clone();

    assert_eq!(previewed, presented);
    // The fence holds, so the deck is three slides rather than four.
    assert_eq!(previewed.as_array().unwrap().len(), 3);
    assert!(
        !previewed.to_string().contains("<script>"),
        "the preview handed back live markup"
    );
}

#[tokio::test]
async fn a_deck_too_large_to_present_is_too_large_to_preview() {
    let host = spawn().await;
    let deck = "a".repeat(300 * 1024);
    let (status, _) = post_json(
        &host,
        "/api/preview",
        serde_json::json!({ "markdown": deck }),
    )
    .await;
    assert_eq!(status, 413);
}

async fn ws_send(socket: &mut Socket, msg: Value) {
    socket
        .send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
}

/// A talk is put up by somebody who is already in the room, so the running
/// order says who is giving it.
#[tokio::test]
async fn a_submitted_talk_carries_the_name_of_whoever_put_it_up() {
    let host = spawn().await;
    let (id, mc) = create(&host, "# Lightning talks").await;

    let mut console = open(&host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "submissions", "open": true }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut phone = open_as(&host, &id, None, "ada-browser").await;
    ws_send(
        &mut phone,
        serde_json::json!({ "type": "set_name", "name": "Ada" }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let res = reqwest::Client::new()
        .post(format!("http://{host}/api/sessions/{id}/talks"))
        .json(&serde_json::json!({
            "title": "",
            "markdown": "# Borrow checking\n\n---\n\n# Two",
            "who": "ada-browser",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "the submission was refused");

    // Read the lineup off a freshly opened socket, which is hydrated with it.
    let mut later = open(&host, &id, Some(&mc)).await;
    let mut seen = None;
    for _ in 0..8 {
        let msg = next_json(&mut later).await;
        if msg["type"] == "lineup" {
            seen = Some(msg);
            break;
        }
    }
    let lineup = seen.expect("no lineup in the opening state");
    let entry = &lineup["items"][0];
    assert_eq!(entry["title"], "Borrow checking");
    assert_eq!(entry["slides"], 2);
    assert_eq!(
        entry["by"], "Ada",
        "the running order lost who is giving the talk"
    );
}

/// The export is what the evening was for, so it has to hold the decks, who
/// gave them, what the room asked, and cues a video editor can use.
#[tokio::test]
async fn the_export_holds_the_evening() {
    let host = spawn().await;
    let (id, mc) = create(&host, "# Lightning talks\n\n---\n\n# Up next").await;
    let mut console = open(&host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "submissions", "open": true }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut phone = open_as(&host, &id, None, "ada-browser").await;
    ws_send(
        &mut phone,
        serde_json::json!({ "type": "set_name", "name": "Ada" }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    reqwest::Client::new()
        .post(format!("http://{host}/api/sessions/{id}/talks"))
        .json(&serde_json::json!({
            "title": "Borrow checking",
            "markdown": "# Borrow checking\n\n---\n\n# It stops hurting",
            "who": "ada-browser",
        }))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    ws_send(
        &mut console,
        serde_json::json!({ "type": "stage", "talk": 1 }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "goto", "index": 1 }),
    )
    .await;
    ws_send(
        &mut phone,
        serde_json::json!({ "type": "ask", "text": "how long" }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(60)).await;

    let bytes = reqwest::get(format!("http://{host}/api/sessions/{id}/export?token={mc}"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|n| zip.by_index(n).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"00-host.md".to_string()), "got {names:?}");
    assert!(
        names.contains(&"01-borrow-checking.md".to_string()),
        "a talk is missing or misnamed: {names:?}"
    );

    let read = |zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str| {
        use std::io::Read;
        let mut out = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
        out
    };

    assert_eq!(
        read(&mut zip, "01-borrow-checking.md"),
        "# Borrow checking\n\n---\n\n# It stops hurting"
    );

    let record: Value = serde_json::from_str(&read(&mut zip, "timeline.json")).unwrap();
    assert_eq!(record["talks"][1]["by"], "Ada");
    assert_eq!(record["talks"][1]["questions"][0]["text"], "how long");
    assert_eq!(record["board"][0]["name"], "Ada");
    let cues = record["timeline"].as_array().unwrap();
    assert_eq!(
        cues.len(),
        2,
        "the timeline is not what the room saw: {cues:?}"
    );
    assert_eq!(cues[0]["slide"], 0);
    assert_eq!(cues[1]["slide"], 1);
    assert_eq!(cues[1]["title"], "Borrow checking");

    // A cue file a video editor can drop straight onto a recording.
    let vtt = read(&mut zip, "slides.vtt");
    assert!(vtt.starts_with("WEBVTT"), "got {vtt}");
    assert!(vtt.contains("--> "), "no cue timings: {vtt}");
    assert!(
        vtt.contains("Borrow checking \u{2014} slide 2"),
        "got {vtt}"
    );
}

#[tokio::test]
async fn an_instance_without_a_deck_offers_the_start_page_nothing() {
    let host = spawn().await;
    let res = reqwest::get(format!("http://{host}/api/starter"))
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
}

#[tokio::test]
async fn the_deck_the_server_was_started_with_reaches_the_start_page() {
    let deck = "# Quiz night\n\n---\n\n# Round one\n";
    let host = spawn_with_deck(deck).await;
    let res = reqwest::get(format!("http://{host}/api/starter"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), deck);
}

/// Opens a room that takes talks and puts one up, the way a phone does.
async fn open_mic(host: &str) -> (String, String) {
    let (id, mc) = create(host, "# Lightning talks").await;
    let mut console = open(host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "submissions", "open": true }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    (id, mc)
}

async fn put_up(host: &str, id: &str, who: &str, markdown: &str) -> (u64, String) {
    let res = reqwest::Client::new()
        .post(format!("http://{host}/api/sessions/{id}/talks"))
        .json(&serde_json::json!({ "title": "", "markdown": markdown, "who": who }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "the submission was refused");
    let json: Value = res.json().await.unwrap();
    (
        json["id"].as_u64().unwrap(),
        json["token"].as_str().unwrap().to_string(),
    )
}

async fn read_talk(host: &str, id: &str, talk: u64, token: &str) -> (u16, Value) {
    let res = reqwest::get(format!(
        "http://{host}/api/sessions/{id}/talks/{talk}?token={token}"
    ))
    .await
    .unwrap();
    let status = res.status().as_u16();
    let body = res.text().await.unwrap();
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

/// The point of putting a talk up early is being able to read it back and fix
/// it before standing up in front of a room.
#[tokio::test]
async fn a_speaker_reads_and_rewrites_their_own_talk_while_they_wait() {
    let host = spawn().await;
    let (id, _) = open_mic(&host).await;
    let (talk, token) = put_up(&host, &id, "ada-browser", "# Borrow checking").await;

    let (status, detail) = read_talk(&host, &id, talk, &token).await;
    assert_eq!(status, 200, "a speaker could not open their own talk");
    assert_eq!(detail["markdown"], "# Borrow checking");
    assert_eq!(detail["position"], 1);
    assert_eq!(detail["dropped"], false);

    let res = reqwest::Client::new()
        .put(format!(
            "http://{host}/api/sessions/{id}/talks/{talk}?token={token}"
        ))
        .json(&serde_json::json!({
            "title": "Borrow checking, shorter",
            "markdown": "# Borrow checking\n\n---\n\n# One rule",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204, "a speaker could not fix their own talk");

    let (_, detail) = read_talk(&host, &id, talk, &token).await;
    assert_eq!(detail["markdown"], "# Borrow checking\n\n---\n\n# One rule");
    assert_eq!(detail["title"], "Borrow checking, shorter");
}

/// The running order is public. The decks behind it are not.
#[tokio::test]
async fn a_phone_cannot_open_a_talk_it_did_not_write() {
    let host = spawn().await;
    let (id, _) = open_mic(&host).await;
    let (talk, _) = put_up(&host, &id, "ada-browser", "# Mine").await;
    let (_, stranger) = put_up(&host, &id, "bob-browser", "# Theirs").await;

    assert_eq!(read_talk(&host, &id, talk, &stranger).await.0, 403);
    assert_eq!(read_talk(&host, &id, talk, "").await.0, 403);
    assert_eq!(
        read_talk(&host, &id, 999, &stranger).await.0,
        404,
        "a talk that never existed was a refusal rather than a miss"
    );

    let res = reqwest::Client::new()
        .put(format!(
            "http://{host}/api/sessions/{id}/talks/{talk}?token={stranger}"
        ))
        .json(&serde_json::json!({ "title": "", "markdown": "# Not yours" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403, "a stranger rewrote somebody else's talk");
}

/// A note about a talk is for the speaker who wrote it. A broadcast reaches a
/// room, so it travels to the one phone that asks with the right token.
#[tokio::test]
async fn the_note_behind_a_drop_reaches_the_speaker_and_not_the_room() {
    let host = spawn().await;
    let (id, mc) = open_mic(&host).await;
    let (talk, token) = put_up(&host, &id, "ada-browser", "# Borrow checking").await;

    let mut phone = open(&host, &id, None).await;
    let mut console = open(&host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "drop", "talk": talk, "note": "Runs twice too long" }),
    )
    .await;

    let mut audience = None;
    for _ in 0..12 {
        let msg = next_json(&mut phone).await;
        if msg["type"] == "lineup" && msg["items"].as_array().unwrap().is_empty() {
            audience = Some(msg);
            break;
        }
    }
    let lineup = audience.expect("the room was never told the talk came off");
    assert_eq!(
        lineup["dropped"].as_array().map(Vec::len),
        Some(0),
        "the room was shown what was pulled from it: {lineup}"
    );

    let (status, detail) = read_talk(&host, &id, talk, &token).await;
    assert_eq!(status, 200);
    assert_eq!(detail["note"], "Runs twice too long");
    assert_eq!(detail["dropped"], true);
    assert_eq!(detail["markdown"], "# Borrow checking");
}

/// Ordering the evening is the host's, and a talk moved on one phone moves on
/// every phone.
#[tokio::test]
async fn the_host_reorders_the_running_order_for_the_whole_room() {
    let host = spawn().await;
    let (id, mc) = open_mic(&host).await;
    let (first, _) = put_up(&host, &id, "ada-browser", "# Ada").await;
    let (second, speaker) = put_up(&host, &id, "bob-browser", "# Bob").await;

    let mut phone = open(&host, &id, None).await;
    let mut console = open(&host, &id, Some(&mc)).await;

    // A speaker's own token is not the host's.
    ws_send(
        &mut console,
        serde_json::json!({ "type": "hand", "talk": second }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut driver = open(&host, &id, Some(&speaker)).await;
    ws_send(
        &mut driver,
        serde_json::json!({ "type": "reorder", "talk": second, "index": 0 }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    ws_send(
        &mut console,
        serde_json::json!({ "type": "reorder", "talk": second, "index": 0 }),
    )
    .await;

    let mut moved = None;
    for _ in 0..12 {
        let msg = next_json(&mut phone).await;
        if msg["type"] == "lineup" && msg["items"][0]["id"] == second {
            moved = Some(msg);
            break;
        }
    }
    let lineup = moved.expect("the room never saw the running order change");
    assert_eq!(lineup["items"][1]["id"], first);
}

/// A talk the host deleted has to read as gone, not as refused: the phone that
/// put it up uses the difference to stop offering to edit something that is
/// no longer there.
#[tokio::test]
async fn a_deleted_talk_reads_as_gone_to_the_phone_that_wrote_it() {
    let host = spawn().await;
    let (id, mc) = open_mic(&host).await;
    let (talk, token) = put_up(&host, &id, "ada-browser", "# Mine").await;
    assert_eq!(read_talk(&host, &id, talk, &token).await.0, 200);

    let mut console = open(&host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "remove", "talk": talk }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(read_talk(&host, &id, talk, &token).await.0, 404);
    let res = reqwest::Client::new()
        .put(format!(
            "http://{host}/api/sessions/{id}/talks/{talk}?token={token}"
        ))
        .json(&serde_json::json!({ "title": "", "markdown": "# Back" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

/// A staged list is a slide that arrives in pieces, and every phone in the room
/// has to arrive at the same piece at the same time.
#[tokio::test]
async fn the_room_follows_the_presenter_through_a_staged_list() {
    let host = spawn().await;
    let (id, token) = create(
        &host,
        "# Why Rust\n\n* No garbage collector\n* No data races",
    )
    .await;

    let mut phone = open(&host, &id, None).await;
    let deck = loop {
        let msg = next_json(&mut phone).await;
        if msg["type"] == "deck" {
            break msg;
        }
    };
    assert_eq!(
        deck["slides"][0]["steps"], 2,
        "the room was not told the staging"
    );
    assert_eq!(deck["step"], 0, "the room opened part way into the slide");
    let html = deck["slides"][0]["html"].as_str().unwrap();
    assert!(html.contains(r#"data-step="1""#), "{html}");

    let mut console = open(&host, &id, Some(&token)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "goto", "index": 0, "step": 1 }),
    )
    .await;

    let moved = loop {
        let msg = next_json(&mut phone).await;
        if msg["type"] == "move" {
            break msg;
        }
    };
    assert_eq!(
        moved["current"], 0,
        "the room changed slide rather than step"
    );
    assert_eq!(moved["step"], 1);
}

/// A step past the end of the slide is clamped. The console sends the position
/// it can see, and an edit under it can leave that past the end.
#[tokio::test]
async fn a_step_past_the_end_of_a_slide_is_clamped() {
    let host = spawn().await;
    let (id, token) = create(&host, "# Plain\n\n- One\n- Two").await;
    let mut phone = open(&host, &id, None).await;
    let mut console = open(&host, &id, Some(&token)).await;

    ws_send(
        &mut console,
        serde_json::json!({ "type": "goto", "index": 0, "step": 9 }),
    )
    .await;

    let moved = loop {
        let msg = next_json(&mut phone).await;
        if msg["type"] == "move" {
            break msg;
        }
    };
    assert_eq!(moved["step"], 0, "a slide that stages nothing took a step");
}

/// A viewer cannot step the room any more than they can move it.
#[tokio::test]
async fn a_viewer_cannot_step_the_slide() {
    let host = spawn().await;
    let (id, _) = create(&host, "* One\n* Two").await;
    let mut phone = open(&host, &id, None).await;
    ws_send(
        &mut phone,
        serde_json::json!({ "type": "goto", "index": 0, "step": 1 }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut later = open(&host, &id, None).await;
    let deck = loop {
        let msg = next_json(&mut later).await;
        if msg["type"] == "deck" {
            break msg;
        }
    };
    assert_eq!(deck["step"], 0, "a viewer walked the room through a list");
}

/// A png of a given size, standing in for whatever a phone camera produced.
fn picture(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::new(width, height);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn put_image(host: &str, id: &str, query: &str, kind: &str, bytes: Vec<u8>) -> (u16, String) {
    let res = reqwest::Client::new()
        .post(format!("http://{host}/api/sessions/{id}/images?{query}"))
        .header("content-type", kind)
        .body(bytes)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.text().await.unwrap())
}

/// An instance that was not told to keep pictures does not keep pictures.
#[tokio::test]
async fn an_instance_without_uploads_takes_none() {
    let host = spawn().await;
    let (id, token) = create(&host, "# Deck").await;

    let (status, _) = put_image(
        &host,
        &id,
        &format!("token={token}&who=ada"),
        "image/png",
        picture(20, 20),
    )
    .await;
    assert_eq!(status, 404, "an instance with uploads off took one");

    let config: Value = reqwest::get(format!("http://{host}/api/config"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(config["uploads"], false, "the page would offer the button");
}

/// A phone photograph is several megabytes and four thousand pixels across.
/// What the room serves has to be neither.
#[tokio::test]
async fn an_uploaded_picture_is_shrunk_before_the_room_can_ask_for_it() {
    let host = spawn_with_uploads().await;
    let (id, token) = create(&host, "# Deck").await;

    let raw = picture(4000, 3000);
    let (status, body) = put_image(
        &host,
        &id,
        &format!("token={token}&who=ada"),
        "image/png",
        raw.clone(),
    )
    .await;
    assert_eq!(status, 201, "the upload was refused: {body}");
    let url = serde_json::from_str::<Value>(&body).unwrap()["url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(url.starts_with(&format!("/i/{id}/")), "{url}");

    let res = reqwest::get(format!("http://{host}{url}")).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    let served = res.bytes().await.unwrap();
    let drawn = image::load_from_memory(&served).unwrap();
    assert_eq!(drawn.width(), 1600, "the long edge was not brought down");
    assert_eq!(drawn.height(), 1200);
    assert!(
        served.len() < raw.len(),
        "the room was served more than was uploaded"
    );
}

#[tokio::test]
async fn a_missing_picture_is_a_miss_and_not_a_panic() {
    let host = spawn_with_uploads().await;
    let (id, _) = create(&host, "# Deck").await;
    let res = reqwest::get(format!("http://{host}/i/{id}/nothinghere"))
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn something_that_is_not_a_picture_is_refused() {
    let host = spawn_with_uploads().await;
    let (id, token) = create(&host, "# Deck").await;

    let (status, _) = put_image(
        &host,
        &id,
        &format!("token={token}&who=ada"),
        "text/markdown",
        b"# not a picture".to_vec(),
    )
    .await;
    assert_eq!(status, 415, "a markdown file was taken as an image");

    let (status, _) = put_image(
        &host,
        &id,
        &format!("token={token}&who=ada"),
        "image/png",
        b"not a png at all".to_vec(),
    )
    .await;
    assert_eq!(status, 400, "bytes that decode to nothing were kept");
}

/// A closed room is nobody's to upload into. An open one takes pictures from
/// the floor, because it is taking the talks that need them.
#[tokio::test]
async fn a_stranger_uploads_only_while_the_room_is_taking_talks() {
    let host = spawn_with_uploads().await;
    let (id, mc) = create(&host, "# Deck").await;

    let (status, _) = put_image(&host, &id, "who=ada", "image/png", picture(10, 10)).await;
    assert_eq!(status, 403, "a closed room took a picture from the floor");

    let mut console = open(&host, &id, Some(&mc)).await;
    ws_send(
        &mut console,
        serde_json::json!({ "type": "submissions", "open": true }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(80)).await;

    let (status, body) = put_image(&host, &id, "who=ada", "image/png", picture(10, 10)).await;
    assert_eq!(status, 201, "an open room refused one: {body}");
}

/// One picture at a time from any one phone.
#[tokio::test]
async fn a_phone_cannot_fill_a_room_with_pictures() {
    let host = spawn_with_uploads().await;
    let (id, token) = create(&host, "# Deck").await;
    let query = format!("token={token}&who=ada");

    let (first, _) = put_image(&host, &id, &query, "image/png", picture(10, 10)).await;
    assert_eq!(first, 201);
    let (again, _) = put_image(&host, &id, &query, "image/png", picture(10, 10)).await;
    assert_eq!(again, 429, "a second picture landed with no wait");
}

/// A deck points at pictures anywhere, so the policy has to let a phone draw
/// them. It still refuses everything a deck has no business loading.
#[tokio::test]
async fn the_policy_allows_a_picture_from_somewhere_else() {
    let host = spawn().await;
    let res = reqwest::get(format!("http://{host}/")).await.unwrap();
    let csp = res.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("img-src 'self' data: https: http:"), "{csp}");
    assert!(csp.contains("script-src 'self'"), "{csp}");
    assert!(csp.contains("object-src 'none'"), "{csp}");
}

/// A look the deck names has to be a thing the room can actually fetch, and
/// nothing else in the process may be fetchable the same way.
#[tokio::test]
async fn a_built_in_theme_and_transition_are_served_by_name() {
    let host = spawn().await;

    for path in [
        "themes/ember.css",
        "themes/paper.css",
        "transitions/fade.css",
    ] {
        let res = reqwest::get(format!("http://{host}/{path}")).await.unwrap();
        assert_eq!(res.status(), 200, "{path} was not served");
        assert_eq!(
            res.headers()["content-type"],
            "text/css",
            "{path} came back as the wrong type"
        );
        assert!(!res.text().await.unwrap().is_empty(), "{path} was empty");
    }
}

#[tokio::test]
async fn a_look_nobody_installed_is_not_found() {
    let host = spawn().await;
    for path in [
        "themes/nope.css",
        "transitions/nope.css",
        "themes/..%2f..%2fCargo.toml",
        "themes/%2e%2e%2fbase.css",
    ] {
        let res = reqwest::get(format!("http://{host}/{path}")).await.unwrap();
        assert_eq!(res.status(), 404, "{path} was served");
    }
}

#[tokio::test]
async fn the_config_lists_every_look_the_room_may_ask_for() {
    let host = spawn().await;
    let config: Value = reqwest::get(format!("http://{host}/api/config"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let names = |key: &str| -> Vec<String> {
        config[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap().to_string())
            .collect()
    };

    let themes = names("themes");
    assert!(themes.iter().any(|t| t == "ember"), "{themes:?}");
    assert!(themes.iter().any(|t| t == "neon"), "{themes:?}");

    // A picker puts this next to the name, so an empty one is a blank row.
    let about = config["themes"][0]["about"].as_str().unwrap();
    assert!(
        !about.is_empty(),
        "the first theme describes itself as nothing"
    );

    let transitions = names("transitions");
    assert!(transitions.iter().any(|t| t == "fade"), "{transitions:?}");
    assert!(
        transitions.iter().any(|t| t == "coverflow"),
        "{transitions:?}"
    );
    assert_eq!(
        transitions.len(),
        34,
        "the marp set plus none: {transitions:?}"
    );
}

#[tokio::test]
async fn a_look_the_operator_added_at_startup_reaches_the_room() {
    let dir = std::env::temp_dir().join(format!("pc-live-theme-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("midnight.css"), ".viewer { --ground: #000; }").unwrap();

    let styles = std::sync::Arc::new(palmcast::styles::load(Some(&dir), None).unwrap());
    let host = serve(routes::router_with(routes::App {
        registry: Registry::new(Duration::from_secs(3600)),
        public_url: None,
        starter: None,
        uploads: false,
        styles,
    }))
    .await;

    let body = reqwest::get(format!("http://{host}/themes/midnight.css"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, ".viewer { --ground: #000; }");

    let config: Value = reqwest::get(format!("http://{host}/api/config"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let themes = config["themes"].as_array().unwrap();
    assert!(
        themes.iter().any(|t| t["name"] == "midnight"),
        "the room was never told: {themes:?}"
    );
}
