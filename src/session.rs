use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;

use crate::deck::{self, Slide};
use crate::wire::ServerMsg;

/// No vowels, so an id cannot spell a word, and no glyphs that look alike when
/// read off a phone screen in a dark room.
const ALPHABET: &[u8] = b"23456789bcdfghjkmnpqrstvwxz";
const ID_LEN: usize = 6;
const TOKEN_LEN: usize = 32;

pub struct Session {
    pub owner_token: String,
    pub markdown: String,
    pub slides: Vec<Slide>,
    pub rev: u64,
    pub current: usize,
    pub viewers: usize,
    pub touched: Instant,
    pub tx: broadcast::Sender<ServerMsg>,
}

impl Session {
    fn snapshot(&self) -> ServerMsg {
        ServerMsg::Deck {
            rev: self.rev,
            current: self.current,
            slides: self.slides.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Registry {
    inner: Arc<Mutex<HashMap<String, Session>>>,
    ttl: Duration,
}

impl Registry {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub fn create(&self, markdown: &str) -> (String, String) {
        let mut map = self.lock();
        let id = loop {
            let candidate = random_string(ID_LEN);
            if !map.contains_key(&candidate) {
                break candidate;
            }
        };
        let token = random_string(TOKEN_LEN);
        let (tx, _) = broadcast::channel(64);
        map.insert(
            id.clone(),
            Session {
                owner_token: token.clone(),
                markdown: markdown.to_string(),
                slides: deck::parse(markdown),
                rev: 1,
                current: 0,
                viewers: 0,
                touched: Instant::now(),
                tx,
            },
        );
        (id, token)
    }

    pub fn exists(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    pub fn snapshot(&self, id: &str) -> Option<ServerMsg> {
        self.lock().get(id).map(Session::snapshot)
    }

    pub fn markdown(&self, id: &str) -> Option<String> {
        self.lock().get(id).map(|s| s.markdown.clone())
    }

    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<ServerMsg>> {
        self.lock().get(id).map(|s| s.tx.subscribe())
    }

    pub fn owns(&self, id: &str, token: &str) -> bool {
        self.lock()
            .get(id)
            .is_some_and(|s| s.owner_token.as_bytes().ct_eq(token.as_bytes()).into())
    }

    /// Returns the message to broadcast, or None when the caller is not the
    /// owner or the index is out of range.
    pub fn goto(&self, id: &str, token: &str, index: usize) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        let owns: bool = session
            .owner_token
            .as_bytes()
            .ct_eq(token.as_bytes())
            .into();
        if !owns || index >= session.slides.len() {
            return None;
        }
        session.current = index;
        session.touched = Instant::now();
        Some(ServerMsg::Move { current: index })
    }

    pub fn replace_deck(&self, id: &str, token: &str, markdown: &str) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        let owns: bool = session
            .owner_token
            .as_bytes()
            .ct_eq(token.as_bytes())
            .into();
        if !owns {
            return None;
        }
        session.markdown = markdown.to_string();
        session.slides = deck::parse(markdown);
        session.rev += 1;
        session.current = session.current.min(session.slides.len() - 1);
        session.touched = Instant::now();
        Some(session.snapshot())
    }

    pub fn join(&self, id: &str) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        session.viewers += 1;
        session.touched = Instant::now();
        Some(ServerMsg::Viewers {
            count: session.viewers,
        })
    }

    pub fn leave(&self, id: &str) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        session.viewers = session.viewers.saturating_sub(1);
        Some(ServerMsg::Viewers {
            count: session.viewers,
        })
    }

    pub fn broadcast(&self, id: &str, msg: ServerMsg) {
        if let Some(session) = self.lock().get(id) {
            let _ = session.tx.send(msg);
        }
    }

    /// Drops sessions nobody has touched inside the TTL. Returns how many went.
    pub fn sweep(&self) -> usize {
        let mut map = self.lock();
        let before = map.len();
        let ttl = self.ttl;
        map.retain(|_, s| s.viewers > 0 || s.touched.elapsed() < ttl);
        before - map.len()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.inner.lock().expect("session registry lock")
    }
}

fn random_string(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}
