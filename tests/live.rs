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
    let query = token.map(|t| format!("?token={t}")).unwrap_or_default();
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
