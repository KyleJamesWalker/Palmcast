use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;

use crate::deck::{self, Slide};
use crate::persist::{PersistedQuestion, PersistedSession};
use crate::wire::{AudienceQuestion, Frame, Reaction, ScoreRow, ServerMsg};

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
/// `who` arrives from the client, so every map keyed by it would grow without
/// limit against a loop of fresh ids. This bounds the room and, with it, how
/// far one client can inflate a tally.
const MAX_PARTICIPANTS: usize = 500;
/// Bounds what one public instance can be made to hold.
const MAX_SESSIONS: usize = 2000;
/// A room bigger than this is not a bar, and every socket costs a broadcast
/// receiver.
const MAX_VIEWERS: usize = 400;
/// A name is a label on a leaderboard, not a field for prose.
const MAX_NAME_CHARS: usize = 24;
/// Bounds one broadcast. Nobody reads past the top of a leaderboard anyway.
const MAX_SCORE_ROWS: usize = 50;
const TOKEN_LEN: usize = 32;

pub struct Session {
    pub owner_token: String,
    pub markdown: String,
    pub slides: Vec<Slide>,
    pub rev: u64,
    pub current: usize,
    pub viewers: usize,
    pub touched: Instant,
    pub tx: broadcast::Sender<Arc<Frame>>,
    /// slide index -> voter id -> chosen option. One vote each, last one wins.
    pub votes: HashMap<usize, HashMap<String, usize>>,
    pub revealed: HashSet<usize>,
    pub last_reaction: HashMap<String, Instant>,
    pub questions: Vec<StoredQuestion>,
    pub last_ask: HashMap<String, Instant>,
    pub next_question_id: u64,
    pub participants: HashSet<String>,
    /// participant id -> the name they chose.
    pub names: HashMap<String, String>,
}

pub struct StoredQuestion {
    pub id: u64,
    pub text: String,
    pub answered: bool,
    pub voters: HashSet<String>,
}

impl Session {
    /// True when this viewer may take part. A viewer already known is always
    /// admitted, so the cap turns away new ids rather than existing ones.
    fn admit(&mut self, who: &str) -> bool {
        if self.participants.contains(who) {
            return true;
        }
        if self.participants.len() >= MAX_PARTICIPANTS {
            return false;
        }
        self.participants.insert(who.to_string());
        true
    }

    /// One point for each revealed question this person got right. Scores are
    /// derived from the votes rather than counted as they arrive, so a late
    /// reveal or a correction cannot leave a stale total behind.
    fn score_table(&self) -> ServerMsg {
        let mut items: Vec<ScoreRow> = self
            .names
            .iter()
            .map(|(who, name)| {
                let score = self
                    .revealed
                    .iter()
                    .filter(|slide| {
                        let Some(question) =
                            self.slides.get(**slide).and_then(|s| s.question.as_ref())
                        else {
                            return false;
                        };
                        self.votes
                            .get(*slide)
                            .and_then(|cast| cast.get(who))
                            .is_some_and(|option| question.correct.contains(option))
                    })
                    .count();
                ScoreRow {
                    name: name.clone(),
                    score,
                }
            })
            .collect();
        items.sort_by(|a, b| b.score.cmp(&a.score).then(a.name.cmp(&b.name)));
        items.truncate(MAX_SCORE_ROWS);
        ServerMsg::Scores { items }
    }

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

    /// `None` when the instance is already holding MAX_SESSIONS.
    pub fn create(&self, markdown: &str) -> Option<(String, String)> {
        let mut map = self.lock();
        if map.len() >= MAX_SESSIONS {
            // Drop anything idle before turning a real room away.
            let ttl = self.ttl;
            map.retain(|_, s| s.viewers > 0 || s.touched.elapsed() < ttl);
            if map.len() >= MAX_SESSIONS {
                return None;
            }
        }
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
                participants: HashSet::new(),
                names: HashMap::new(),
            },
        );
        Some((id, token))
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

    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<Arc<Frame>>> {
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
        let rebuilt = deck::parse(markdown);
        // A typo fixed on slide one must not throw away a quiz in progress, so
        // only the questions whose options actually changed lose their votes.
        let intact: HashSet<usize> = rebuilt
            .iter()
            .enumerate()
            .filter(|(index, slide)| {
                match (
                    session.slides.get(*index).and_then(|s| s.question.as_ref()),
                    slide.question.as_ref(),
                ) {
                    (Some(before), Some(after)) => before.options == after.options,
                    _ => false,
                }
            })
            .map(|(index, _)| index)
            .collect();
        session.votes.retain(|slide, _| intact.contains(slide));
        session.revealed.retain(|slide| intact.contains(slide));
        session.slides = rebuilt;
        session.rev += 1;
        session.current = session.current.min(session.slides.len() - 1);
        session.touched = Instant::now();
        Some(session.snapshot())
    }

    /// `None` when the room is full, which the caller turns into a closed
    /// socket rather than a silent viewer who sees nothing.
    pub fn join(&self, id: &str) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        if session.viewers >= MAX_VIEWERS {
            return None;
        }
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

    /// Serializes once for the whole room rather than once per socket.
    pub fn broadcast(&self, id: &str, msg: ServerMsg) {
        let frame = Frame::new(&msg);
        if let Some(session) = self.lock().get(id) {
            let _ = session.tx.send(frame);
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
        if option >= width || !session.admit(who) {
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
        if !session.admit(who) {
            return None;
        }
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
        if !session.admit(who) {
            return None;
        }
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
        if !session.admit(who) {
            return None;
        }
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

    pub fn set_name(&self, id: &str, who: &str, name: &str) -> Option<ServerMsg> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return None;
        }
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        if !session.admit(who) {
            return None;
        }
        session.names.insert(who.to_string(), name.to_string());
        session.touched = Instant::now();
        Some(session.score_table())
    }

    pub fn scores(&self, id: &str) -> Option<ServerMsg> {
        self.lock().get(id).map(Session::score_table)
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

    pub fn viewers(&self) -> usize {
        self.lock().values().map(|s| s.viewers).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Everything worth carrying across a restart.
    pub fn export(&self) -> Vec<PersistedSession> {
        self.lock()
            .iter()
            .map(|(id, s)| PersistedSession {
                id: id.clone(),
                owner_token: s.owner_token.clone(),
                markdown: s.markdown.clone(),
                current: s.current,
                rev: s.rev,
                votes: s.votes.clone(),
                revealed: s.revealed.clone(),
                questions: s
                    .questions
                    .iter()
                    .map(|q| PersistedQuestion {
                        id: q.id,
                        text: q.text.clone(),
                        answered: q.answered,
                        voters: q.voters.clone(),
                    })
                    .collect(),
                next_question_id: s.next_question_id,
                names: s.names.clone(),
                participants: s.participants.clone(),
                idle_seconds: s.touched.elapsed().as_secs(),
            })
            .collect()
    }

    /// Returns how many came back. Viewer counts start at zero, because nobody
    /// is connected to a process that has just started.
    pub fn import(&self, saved: Vec<PersistedSession>) -> usize {
        let mut map = self.lock();
        let mut restored = 0;
        let now = Instant::now();
        for item in saved.into_iter().take(MAX_SESSIONS) {
            let slides = deck::parse(&item.markdown);
            let current = item.current.min(slides.len().saturating_sub(1));
            let (tx, _) = broadcast::channel(64);
            // Carrying the age forward means the next sweep drops whatever had
            // already run out, rather than the restart granting it a new life.
            let touched = now
                .checked_sub(Duration::from_secs(item.idle_seconds))
                .unwrap_or(now);
            map.insert(
                item.id,
                Session {
                    owner_token: item.owner_token,
                    markdown: item.markdown,
                    slides,
                    rev: item.rev,
                    current,
                    viewers: 0,
                    touched,
                    tx,
                    votes: item.votes,
                    revealed: item.revealed,
                    last_reaction: HashMap::new(),
                    questions: item
                        .questions
                        .into_iter()
                        .map(|q| StoredQuestion {
                            id: q.id,
                            text: q.text,
                            answered: q.answered,
                            voters: q.voters,
                        })
                        .collect(),
                    last_ask: HashMap::new(),
                    next_question_id: item.next_question_id.max(1),
                    participants: item.participants,
                    names: item.names,
                },
            );
            restored += 1;
        }
        restored
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Reaction;

    fn registry() -> Registry {
        Registry::new(Duration::from_secs(3600))
    }

    #[test]
    fn a_participant_cap_bounds_a_room() {
        let reg = registry();
        let (id, _) = reg.create("# q\n\n- [ ] a\n- [x] b").unwrap();

        for n in 0..MAX_PARTICIPANTS {
            assert!(
                reg.answer(&id, 0, &format!("who{n}"), 0).is_some(),
                "voter {n} inside the cap was turned away"
            );
        }
        assert!(
            reg.answer(&id, 0, "one-too-many", 0).is_none(),
            "a fresh id past the cap still voted"
        );
        // Someone already admitted keeps taking part.
        assert!(reg.answer(&id, 0, "who0", 1).is_some());
    }

    #[test]
    fn the_cap_does_not_grow_the_maps_past_it() {
        let reg = registry();
        let (id, _) = reg.create("# hi").unwrap();
        for n in 0..(MAX_PARTICIPANTS + 50) {
            let _ = reg.react(&id, &format!("who{n}"), Reaction::Clap);
        }
        let map = reg.lock();
        let session = map.get(&id).unwrap();
        assert_eq!(session.participants.len(), MAX_PARTICIPANTS);
        assert!(session.last_reaction.len() <= MAX_PARTICIPANTS);
    }

    #[test]
    fn a_restored_session_rebuilds_its_slides_and_clamps_the_position() {
        let reg = registry();
        let saved = vec![crate::persist::PersistedSession {
            id: "abc123".into(),
            owner_token: "tok".into(),
            // Two slides, but the saved position points past them.
            markdown: "# One\n\n---\n\n# Two".into(),
            current: 9,
            rev: 4,
            votes: Default::default(),
            revealed: Default::default(),
            questions: Vec::new(),
            next_question_id: 0,
            names: Default::default(),
            participants: Default::default(),
            idle_seconds: 0,
        }];
        assert_eq!(reg.import(saved), 1);

        let map = reg.lock();
        let session = map.get("abc123").unwrap();
        assert_eq!(session.slides.len(), 2);
        assert_eq!(session.current, 1, "a stale position was not clamped");
        assert_eq!(session.viewers, 0);
        // A zero id would collide with the first question asked after a restart.
        assert_eq!(session.next_question_id, 1);
    }

    #[test]
    fn an_instance_stops_creating_sessions_at_the_cap() {
        let reg = registry();
        let mut made = 0;
        for _ in 0..(MAX_SESSIONS + 10) {
            if reg.create("# hi").is_some() {
                made += 1;
            }
        }
        assert_eq!(made, MAX_SESSIONS, "the instance created {made} sessions");
        assert!(reg.create("# hi").is_none());
    }

    #[test]
    fn a_restart_does_not_resurrect_a_room_that_had_expired() {
        let ttl = Duration::from_secs(60);
        let before = Registry::new(ttl);
        let (id, _token) = before.create("# Old room").unwrap();

        // The room has sat idle for well past its life.
        {
            let mut map = before.lock();
            let session = map.get_mut(&id).unwrap();
            session.touched = Instant::now() - Duration::from_secs(600);
        }

        let saved = before.export();
        let after = Registry::new(ttl);
        after.import(saved);

        assert_eq!(
            after.sweep(),
            1,
            "a room idle past its ttl came back alive after a restart"
        );
        assert!(!after.exists(&id));
    }

    #[test]
    fn a_restart_keeps_the_remaining_life_of_a_live_room() {
        let ttl = Duration::from_secs(600);
        let before = Registry::new(ttl);
        let (id, _token) = before.create("# Live room").unwrap();
        {
            let mut map = before.lock();
            map.get_mut(&id).unwrap().touched = Instant::now() - Duration::from_secs(60);
        }

        let after = Registry::new(ttl);
        after.import(before.export());

        assert_eq!(after.sweep(), 0, "a live room was swept after a restart");
        assert!(after.exists(&id));
    }
}
