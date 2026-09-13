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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, routes::router(registry))
            .await
            .unwrap();
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
            r#"{"type":"answer","slide":0,"option":1}"#.into(),
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
            r#"{"type":"answer","slide":0,"option":1}"#.into(),
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
                format!(r#"{{"type":"answer","slide":0,"option":{option}}}"#).into(),
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
            r#"{"type":"answer","slide":0,"option":0}"#.into(),
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
            r#"{"type":"answer","slide":0,"option":0}"#.into(),
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
            r#"{"type":"answer","slide":0,"option":1}"#.into(),
        ))
        .await
        .unwrap();
    player
        .send(Message::Text(
            r#"{"type":"answer","slide":1,"option":1}"#.into(),
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
            r#"{"type":"answer","slide":0,"option":1}"#.into(),
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
            r#"{"type":"answer","slide":1,"option":1}"#.into(),
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
            r#"{"type":"answer","slide":1,"option":1}"#.into(),
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
