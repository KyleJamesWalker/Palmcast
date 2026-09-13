use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;

use crate::deck::{self, Slide};
use crate::wire::{AudienceQuestion, Reaction, ServerMsg};

/// No vowels, so an id cannot spell a word, and no glyphs that look alike when
/// read off a phone screen in a dark room.
const ALPHABET: &[u8] = b"23456789bcdfghjkmnpqrstvwxz";
const ID_LEN: usize = 6;
const REACTION_GAP: Duration = Duration::from_millis(400);
const ASK_GAP: Duration = Duration::from_secs(3);
/// Long enough for a real question, short enough that nobody pastes a speech.
const MAX_QUESTION_CHARS: usize = 280;
/// Bounds the memory one session can take from a public instance.
const MAX_QUESTIONS: usize = 200;
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
    /// slide index -> voter id -> chosen option. One vote each, last one wins.
    pub votes: HashMap<usize, HashMap<String, usize>>,
    pub revealed: HashSet<usize>,
    pub last_reaction: HashMap<String, Instant>,
    pub questions: Vec<StoredQuestion>,
    pub last_ask: HashMap<String, Instant>,
    pub next_question_id: u64,
}

pub struct StoredQuestion {
    pub id: u64,
    pub text: String,
    pub answered: bool,
    pub voters: HashSet<String>,
}

impl Session {
    /// Most wanted first, with anything the presenter has marked answered sunk
    /// to the bottom rather than deleted.
    fn question_list(&self) -> ServerMsg {
        let mut items: Vec<AudienceQuestion> = self
            .questions
            .iter()
            .map(|q| AudienceQuestion {
                id: q.id,
                text: q.text.clone(),
                votes: q.voters.len(),
                answered: q.answered,
            })
            .collect();
        items.sort_by(|a, b| {
            a.answered
                .cmp(&b.answered)
                .then(b.votes.cmp(&a.votes))
                .then(a.id.cmp(&b.id))
        });
        ServerMsg::Questions { items }
    }

    fn counts(&self, slide: usize) -> (Vec<usize>, usize) {
        let width = self
            .slides
            .get(slide)
            .and_then(|s| s.question.as_ref())
            .map(|q| q.options.len())
            .unwrap_or(0);
        let mut counts = vec![0usize; width];
        let cast = self.votes.get(&slide);
        let total = cast.map(HashMap::len).unwrap_or(0);
        if let Some(cast) = cast {
            for option in cast.values() {
                if let Some(slot) = counts.get_mut(*option) {
                    *slot += 1;
                }
            }
        }
        (counts, total)
    }

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
                votes: HashMap::new(),
                revealed: HashSet::new(),
                last_reaction: HashMap::new(),
                questions: Vec::new(),
                last_ask: HashMap::new(),
                next_question_id: 1,
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
        // The options may have changed under them, so old votes no longer mean
        // anything.
        session.votes.clear();
        session.revealed.clear();
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

    /// Records one vote and returns the tally for the presenter. A voter who
    /// answers twice replaces their own vote rather than adding one.
    pub fn answer(&self, id: &str, slide: usize, who: &str, option: usize) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        if session.revealed.contains(&slide) {
            return None;
        }
        let width = session
            .slides
            .get(slide)
            .and_then(|s| s.question.as_ref())
            .map(|q| q.options.len())?;
        if option >= width {
            return None;
        }
        session
            .votes
            .entry(slide)
            .or_default()
            .insert(who.to_string(), option);
        session.touched = Instant::now();
        let (counts, total) = session.counts(slide);
        Some(ServerMsg::Tally {
            slide,
            counts,
            total,
        })
    }

    /// One reaction per viewer per REACTION_GAP. A thumb can move faster than
    /// a room can read, and an unthrottled tap is a denial of service on the
    /// broadcast channel.
    pub fn react(&self, id: &str, who: &str, kind: Reaction) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        let now = Instant::now();
        if let Some(last) = session.last_reaction.get(who)
            && now.duration_since(*last) < REACTION_GAP
        {
            return None;
        }
        session.last_reaction.insert(who.to_string(), now);
        session.touched = now;
        Some(ServerMsg::React { kind })
    }

    pub fn ask(&self, id: &str, who: &str, text: &str) -> Option<ServerMsg> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_QUESTION_CHARS {
            return None;
        }
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        if session.questions.len() >= MAX_QUESTIONS {
            return None;
        }
        let now = Instant::now();
        if let Some(last) = session.last_ask.get(who)
            && now.duration_since(*last) < ASK_GAP
        {
            return None;
        }
        session.last_ask.insert(who.to_string(), now);

        let question_id = session.next_question_id;
        session.next_question_id += 1;
        // The asker's own vote, so a question starts at one rather than zero.
        let voters = HashSet::from([who.to_string()]);
        session.questions.push(StoredQuestion {
            id: question_id,
            text: text.to_string(),
            answered: false,
            voters,
        });
        session.touched = now;
        Some(session.question_list())
    }

    pub fn upvote(&self, id: &str, who: &str, question: u64) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        let found = session.questions.iter_mut().find(|q| q.id == question)?;
        // A set, so a second tap from the same browser is not a second vote.
        if !found.voters.insert(who.to_string()) {
            return None;
        }
        session.touched = Instant::now();
        Some(session.question_list())
    }

    pub fn mark_answered(&self, id: &str, token: &str, question: u64) -> Option<ServerMsg> {
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
        let found = session.questions.iter_mut().find(|q| q.id == question)?;
        found.answered = true;
        session.touched = Instant::now();
        Some(session.question_list())
    }

    pub fn questions(&self, id: &str) -> Option<ServerMsg> {
        self.lock().get(id).map(Session::question_list)
    }

    pub fn reveal(&self, id: &str, token: &str, slide: usize) -> Option<ServerMsg> {
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
        let correct = session
            .slides
            .get(slide)
            .and_then(|s| s.question.as_ref())
            .map(|q| q.correct.clone())?;
        session.revealed.insert(slide);
        session.touched = Instant::now();
        let (counts, total) = session.counts(slide);
        Some(ServerMsg::Reveal {
            slide,
            correct,
            counts,
            total,
        })
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
