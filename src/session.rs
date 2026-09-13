use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;

use crate::deck::{self, Slide};
use crate::persist::{Choice, PersistedQuestion, PersistedSession};
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

/// What a socket or a request is allowed to do.
///
/// One person drives, so the room never watches two people fight over the
/// slide. A co-host works on the deck while that happens, which is the point:
/// writing the next question from the floor without taking the room with you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Viewer,
    CoHost,
    Mc,
}

impl Role {
    pub fn drives(self) -> bool {
        self == Role::Mc
    }

    pub fn edits(self) -> bool {
        matches!(self, Role::Mc | Role::CoHost)
    }
}

pub struct Session {
    pub owner_token: String,
    /// Minted with the session so it is never absent, and only useful to
    /// somebody the presenter hands it to.
    pub cohost_token: String,
    pub markdown: String,
    pub slides: Vec<Slide>,
    pub rev: u64,
    pub current: usize,
    pub viewers: usize,
    pub touched: Instant,
    pub tx: broadcast::Sender<Arc<Frame>>,
    /// slide index -> voter id -> the options they chose. One selection each,
    /// and a later one replaces it rather than adding to it.
    pub votes: HashMap<usize, HashMap<String, Vec<usize>>>,
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
                        // The point is for the answer, not for one part of it,
                        // so the selection has to be exactly right.
                        self.votes
                            .get(*slide)
                            .and_then(|cast| cast.get(who))
                            .is_some_and(|chosen| {
                                let mut want = question.correct.clone();
                                want.sort_unstable();
                                *chosen == want
                            })
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
        // People, not choices: somebody picking three options is still one vote.
        let total = cast.map(HashMap::len).unwrap_or(0);
        if let Some(cast) = cast {
            for chosen in cast.values() {
                for option in chosen {
                    if let Some(slot) = counts.get_mut(*option) {
                        *slot += 1;
                    }
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

#[derive(Debug, PartialEq, Eq)]
pub enum EditError {
    Forbidden,
    Gone,
    /// Somebody else saved first. `current` is the revision to rebase on.
    Stale {
        current: u64,
    },
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
        let cohost = random_string(TOKEN_LEN);
        let (tx, _) = broadcast::channel(64);
        map.insert(
            id.clone(),
            Session {
                owner_token: token.clone(),
                cohost_token: cohost.clone(),
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

    /// Constant time both ways, so a wrong token cannot be narrowed by timing.
    pub fn role(&self, id: &str, token: &str) -> Role {
        let map = self.lock();
        let Some(session) = map.get(id) else {
            return Role::Viewer;
        };
        let mc: bool = session
            .owner_token
            .as_bytes()
            .ct_eq(token.as_bytes())
            .into();
        let cohost: bool = session
            .cohost_token
            .as_bytes()
            .ct_eq(token.as_bytes())
            .into();
        match (mc, cohost) {
            (true, _) => Role::Mc,
            (_, true) => Role::CoHost,
            _ => Role::Viewer,
        }
    }

    pub fn cohost_token(&self, id: &str, token: &str) -> Option<String> {
        let map = self.lock();
        let session = map.get(id)?;
        let mc: bool = session
            .owner_token
            .as_bytes()
            .ct_eq(token.as_bytes())
            .into();
        mc.then(|| session.cohost_token.clone())
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

    pub fn replace_deck(
        &self,
        id: &str,
        role: Role,
        base_rev: Option<u64>,
        markdown: &str,
    ) -> Result<ServerMsg, EditError> {
        if !role.edits() {
            return Err(EditError::Forbidden);
        }
        let mut map = self.lock();
        let session = map.get_mut(id).ok_or(EditError::Gone)?;
        // Two people can be editing at once. Whoever saves second is told,
        // rather than quietly writing over the first.
        if let Some(base) = base_rev
            && base != session.rev
        {
            return Err(EditError::Stale {
                current: session.rev,
            });
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
        Ok(session.snapshot())
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
    pub fn answer(
        &self,
        id: &str,
        slide: usize,
        who: &str,
        options: &[usize],
    ) -> Option<ServerMsg> {
        let mut map = self.lock();
        let session = map.get_mut(id)?;
        if session.revealed.contains(&slide) {
            return None;
        }
        let question = session
            .slides
            .get(slide)
            .and_then(|s| s.question.as_ref())?;
        let width = question.options.len();
        // A single answer question takes one pick however many arrive.
        let limit = if question.multi { width } else { 1 };

        let mut chosen: Vec<usize> = options.iter().copied().filter(|o| *o < width).collect();
        chosen.sort_unstable();
        chosen.dedup();
        if chosen.is_empty() || chosen.len() > limit {
            return None;
        }
        if !session.admit(who) {
            return None;
        }

        session
            .votes
            .entry(slide)
            .or_default()
            .insert(who.to_string(), chosen);
        session.touched = Instant::now();
        let (counts, total) = session.counts(slide);
        Some(ServerMsg::Tally {
            slide,
            counts,
            total,
        })
    }

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

    pub fn mark_answered(&self, id: &str, role: Role, question: u64) -> Option<ServerMsg> {
        if !role.edits() {
            return None;
        }
        let mut map = self.lock();
        let session = map.get_mut(id)?;
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

    /// Every slide that already holds votes, so a console opened part way
    /// through a round knows where the room stands. Presenter only, on the same
    /// footing as a live tally.
    pub fn tallies(&self, id: &str) -> Vec<ServerMsg> {
        let map = self.lock();
        let Some(session) = map.get(id) else {
            return Vec::new();
        };
        let mut slides: Vec<usize> = session.votes.keys().copied().collect();
        slides.sort_unstable();
        slides
            .into_iter()
            .filter_map(|slide| {
                let (counts, total) = session.counts(slide);
                (total > 0).then_some(ServerMsg::Tally {
                    slide,
                    counts,
                    total,
                })
            })
            .collect()
    }

    /// Every answer the presenter has already opened.
    ///
    /// A phone that drops and comes back, or somebody arriving late, would
    /// otherwise sit on a question the rest of the room has already been shown
    /// the answer to. Unlike a tally this is for everyone, because the point of
    /// a reveal is that the answer is now public.
    pub fn reveals(&self, id: &str) -> Vec<ServerMsg> {
        let map = self.lock();
        let Some(session) = map.get(id) else {
            return Vec::new();
        };
        let mut slides: Vec<usize> = session.revealed.iter().copied().collect();
        slides.sort_unstable();
        slides
            .into_iter()
            .filter_map(|slide| {
                let correct = session
                    .slides
                    .get(slide)
                    .and_then(|s| s.question.as_ref())
                    .map(|q| q.correct.clone())?;
                let (counts, total) = session.counts(slide);
                Some(ServerMsg::Reveal {
                    slide,
                    correct,
                    counts,
                    total,
                })
            })
            .collect()
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
                cohost_token: s.cohost_token.clone(),
                markdown: s.markdown.clone(),
                current: s.current,
                rev: s.rev,
                votes: s
                    .votes
                    .iter()
                    .map(|(slide, cast)| {
                        let cast = cast
                            .iter()
                            .map(|(who, chosen)| (who.clone(), Choice::from(chosen.clone())))
                            .collect();
                        (*slide, cast)
                    })
                    .collect(),
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
            // Reparsed here, not restored, so a change to the parser can give
            // the same markdown a different shape. Anything held against a slide
            // position has to be checked against the deck that actually came
            // back, or it describes a slide that is no longer there.
            let slides = deck::parse(&item.markdown);
            let current = item.current.min(slides.len().saturating_sub(1));
            let mut votes: HashMap<usize, HashMap<String, Vec<usize>>> = item
                .votes
                .into_iter()
                .map(|(slide, cast)| {
                    let cast = cast
                        .into_iter()
                        .map(|(who, choice)| {
                            let mut chosen = choice.into_vec();
                            chosen.sort_unstable();
                            chosen.dedup();
                            (who, chosen)
                        })
                        .collect();
                    (slide, cast)
                })
                .collect();
            votes.retain(|slide, _| *slide < slides.len());
            // A vote for an option the deck no longer has means nothing.
            for (slide, cast) in votes.iter_mut() {
                let width = slides
                    .get(*slide)
                    .and_then(|s| s.question.as_ref())
                    .map(|q| q.options.len())
                    .unwrap_or(0);
                cast.retain(|_, chosen| {
                    chosen.iter().all(|option| *option < width) && !chosen.is_empty()
                });
            }
            votes.retain(|_, cast| !cast.is_empty());
            let mut revealed = item.revealed;
            revealed.retain(|slide| *slide < slides.len());
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
                    // An old file has none, and an empty token must never be a
                    // key that works.
                    cohost_token: if item.cohost_token.is_empty() {
                        random_string(TOKEN_LEN)
                    } else {
                        item.cohost_token
                    },
                    markdown: item.markdown,
                    slides,
                    rev: item.rev,
                    current,
                    viewers: 0,
                    touched,
                    tx,
                    votes,
                    revealed,
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
                reg.answer(&id, 0, &format!("who{n}"), &[0]).is_some(),
                "voter {n} inside the cap was turned away"
            );
        }
        assert!(
            reg.answer(&id, 0, "one-too-many", &[0]).is_none(),
            "a fresh id past the cap still voted"
        );
        // Someone already admitted keeps taking part.
        assert!(reg.answer(&id, 0, "who0", &[1]).is_some());
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
            cohost_token: "co".into(),
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

    #[test]
    fn a_quiz_in_progress_comes_back_whole() {
        let before = Registry::new(Duration::from_secs(3600));
        let (id, mc) = before
            .create("# Q one\n\n- [ ] a\n- [x] b\n\n---\n\n# Q two\n\n- [x] c\n- [ ] d")
            .unwrap();

        // Two players, one right and one wrong on the first question.
        before.set_name(&id, "sam", "Sam").unwrap();
        before.set_name(&id, "alex", "Alex").unwrap();
        before.answer(&id, 0, "sam", &[1]).unwrap();
        before.answer(&id, 0, "alex", &[0]).unwrap();
        before.reveal(&id, &mc, 0).unwrap();
        // A vote on a question the mc has not opened yet.
        before.answer(&id, 1, "sam", &[0]).unwrap();

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // The opened answer is still open.
        let reveals = after.reveals(&id);
        assert_eq!(reveals.len(), 1, "the reveal did not survive");
        match &reveals[0] {
            ServerMsg::Reveal {
                slide,
                correct,
                total,
                ..
            } => {
                assert_eq!(*slide, 0);
                assert_eq!(correct, &vec![1]);
                assert_eq!(*total, 2, "the votes did not survive");
            }
            other => panic!("expected a reveal, got {other:?}"),
        }

        // The unopened question kept its vote and stayed shut.
        let tallies = after.tallies(&id);
        assert_eq!(tallies.len(), 2, "a slide with votes lost its tally");

        // Scores recompute from the votes, so only the opened question counts.
        let ServerMsg::Scores { items } = after.scores(&id).unwrap() else {
            panic!("no scores");
        };
        let sam = items.iter().find(|r| r.name == "Sam").expect("Sam is gone");
        let alex = items
            .iter()
            .find(|r| r.name == "Alex")
            .expect("Alex is gone");
        assert_eq!(
            sam.score, 1,
            "a right answer stopped counting after a restart"
        );
        assert_eq!(alex.score, 0, "a wrong answer started counting");
    }

    #[test]
    fn opening_an_answer_after_a_restart_scores_the_votes_cast_before_it() {
        let before = Registry::new(Duration::from_secs(3600));
        let (id, mc) = before.create("# Q\n\n- [ ] a\n- [x] b").unwrap();
        before.set_name(&id, "sam", "Sam").unwrap();
        before.answer(&id, 0, "sam", &[1]).unwrap();

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // The same mc token still opens it, and the vote from before counts.
        after
            .reveal(&id, &mc, 0)
            .expect("the mc token stopped working");
        let ServerMsg::Scores { items } = after.scores(&id).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(
            items[0].score, 1,
            "a vote cast before the restart did not score"
        );
    }

    #[test]
    fn state_pointing_past_the_deck_does_not_survive_a_restore() {
        // What a parser change does: the same markdown now yields fewer slides,
        // so votes and reveals saved against the old positions no longer
        // describe anything. This shape is exactly the code fence fix.
        let saved = vec![crate::persist::PersistedSession {
            id: "abc123".into(),
            owner_token: "tok".into(),
            cohost_token: "co".into(),
            markdown: "# Only one slide now\n\n- [ ] a\n- [x] b".into(),
            current: 0,
            rev: 4,
            votes: HashMap::from([
                (
                    0,
                    HashMap::from([("sam".to_string(), crate::persist::Choice::One(0))]),
                ),
                (
                    5,
                    HashMap::from([("sam".to_string(), crate::persist::Choice::One(1))]),
                ),
            ]),
            revealed: HashSet::from([0, 5]),
            questions: Vec::new(),
            next_question_id: 1,
            names: Default::default(),
            participants: Default::default(),
            idle_seconds: 0,
        }];

        let reg = Registry::new(Duration::from_secs(3600));
        assert_eq!(reg.import(saved), 1);

        let map = reg.lock();
        let session = map.get("abc123").unwrap();
        assert_eq!(session.slides.len(), 1);
        assert!(
            !session.votes.contains_key(&5),
            "votes survived for a slide that no longer exists"
        );
        assert!(
            !session.revealed.contains(&5),
            "a reveal survived for a slide that no longer exists"
        );
        // A vote for an option the slide no longer offers goes too.
        assert!(
            !session
                .votes
                .values()
                .any(|cast| cast.values().any(|chosen| chosen.iter().any(|o| *o >= 2))),
            "a vote survived for an option that is not there"
        );
        // The slide that does still exist keeps its state.
        assert!(session.votes.contains_key(&0));
        assert!(session.revealed.contains(&0));
    }

    #[test]
    fn the_edit_conflict_guard_still_holds_after_a_restart() {
        let before = Registry::new(Duration::from_secs(3600));
        let (id, mc) = before.create("# First").unwrap();
        before
            .replace_deck(&id, Role::Mc, Some(1), "# Second")
            .expect("the first edit should land");

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // Somebody who opened the deck before that edit still has revision 1.
        assert!(
            matches!(
                after.replace_deck(&id, Role::Mc, Some(1), "# Stale"),
                Err(EditError::Stale { current: 2 })
            ),
            "a stale save was accepted after a restart"
        );
        // And somebody current still saves.
        assert!(
            after
                .replace_deck(&id, Role::Mc, Some(2), "# Current")
                .is_ok()
        );
        let _ = mc;
    }

    #[test]
    fn replacing_the_deck_leaves_the_questions_alone() {
        // Question ids are their own sequence, unrelated to slide positions, so
        // an edit has no business touching them.
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create("# One\n\n---\n\n# Two").unwrap();
        reg.ask(&id, "sam", "Why not Go?").unwrap();
        reg.ask(&id, "alex", "How fast is it?").unwrap();

        let ServerMsg::Questions { items } = reg.questions(&id).unwrap() else {
            panic!("no questions");
        };
        let ids: Vec<u64> = items.iter().map(|q| q.id).collect();

        reg.replace_deck(&id, Role::Mc, None, "# Rewritten entirely")
            .unwrap();

        let ServerMsg::Questions { items } = reg.questions(&id).unwrap() else {
            panic!("no questions after the edit");
        };
        assert_eq!(items.len(), 2, "an edit removed questions from the floor");
        assert_eq!(
            items.iter().map(|q| q.id).collect::<Vec<_>>(),
            ids,
            "an edit renumbered the questions"
        );
        assert!(items.iter().any(|q| q.text == "Why not Go?"));
    }

    #[test]
    fn a_reaction_limit_is_per_person_and_not_carried_across_a_restart() {
        let before = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = before.create("# Deck").unwrap();
        assert!(before.react(&id, "sam", Reaction::Clap).is_some());
        // Immediately again is too soon.
        assert!(before.react(&id, "sam", Reaction::Clap).is_none());
        // Somebody else is unaffected.
        assert!(before.react(&id, "alex", Reaction::Clap).is_some());

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());
        // A restart is not a punishment: the gap does not survive it.
        assert!(
            after.react(&id, "sam", Reaction::Clap).is_some(),
            "a restart left somebody unable to react"
        );
    }

    const MULTI: &str = "# Which tracks?\n\n- [x] AI\n- [x] Data Engineering\n- [x] Architecture\n- [ ] Quantum Game Boy";

    #[test]
    fn a_multi_answer_question_scores_only_the_whole_set() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, mc) = reg.create(MULTI).unwrap();
        for who in ["ada", "bo", "cy", "di"] {
            reg.set_name(&id, who, who).unwrap();
        }

        reg.answer(&id, 0, "ada", &[0, 1, 2]).unwrap(); // exactly right
        reg.answer(&id, 0, "bo", &[0]).unwrap(); // one of three
        reg.answer(&id, 0, "cy", &[0, 1, 2, 3]).unwrap(); // all four
        reg.answer(&id, 0, "di", &[3]).unwrap(); // the joke answer
        reg.reveal(&id, &mc, 0).unwrap();

        let ServerMsg::Scores { items } = reg.scores(&id).unwrap() else {
            panic!("no scores");
        };
        let score = |name: &str| items.iter().find(|r| r.name == name).unwrap().score;
        assert_eq!(score("ada"), 1, "the exact set did not score");
        assert_eq!(score("bo"), 0, "a partial answer scored");
        assert_eq!(score("cy"), 0, "picking everything scored");
        assert_eq!(score("di"), 0);
    }

    #[test]
    fn the_order_options_are_picked_in_does_not_matter() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, mc) = reg.create(MULTI).unwrap();
        reg.set_name(&id, "ada", "Ada").unwrap();
        reg.answer(&id, 0, "ada", &[2, 0, 1]).unwrap();
        reg.reveal(&id, &mc, 0).unwrap();

        let ServerMsg::Scores { items } = reg.scores(&id).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(
            items[0].score, 1,
            "a correct answer in another order missed"
        );
    }

    #[test]
    fn a_single_answer_question_still_takes_one_pick() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg
            .create("# One right\n\n- [ ] a\n- [x] b\n- [ ] c")
            .unwrap();
        assert!(reg.answer(&id, 0, "ada", &[1]).is_some());
        assert!(
            reg.answer(&id, 0, "bo", &[0, 1]).is_none(),
            "two picks landed on a single answer question"
        );
    }

    #[test]
    fn a_tally_counts_people_once_however_many_they_pick() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create(MULTI).unwrap();
        reg.answer(&id, 0, "ada", &[0, 1, 2]).unwrap();
        reg.answer(&id, 0, "bo", &[0]).unwrap();

        let map = reg.lock();
        let (counts, total) = map.get(&id).unwrap().counts(0);
        drop(map);
        assert_eq!(counts, vec![2, 1, 1, 0], "options were not counted each");
        assert_eq!(total, 2, "a voter picking three counted as three people");
    }

    #[test]
    fn an_empty_or_impossible_selection_is_refused() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create(MULTI).unwrap();
        assert!(
            reg.answer(&id, 0, "ada", &[]).is_none(),
            "an empty pick landed"
        );
        assert!(
            reg.answer(&id, 0, "ada", &[9]).is_none(),
            "a pick past the options landed"
        );
        // A repeated option is one option, not two.
        assert!(reg.answer(&id, 0, "ada", &[1, 1, 1]).is_some());
        let map = reg.lock();
        assert_eq!(map.get(&id).unwrap().votes[&0]["ada"], vec![1]);
    }

    #[test]
    fn a_question_says_whether_several_answers_are_right() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create(MULTI).unwrap();
        let ServerMsg::Deck { slides, .. } = reg.snapshot(&id).unwrap() else {
            panic!("no deck");
        };
        let q = slides[0].question.as_ref().unwrap();
        assert!(q.multi, "a multi answer question did not say so");

        // And the room is told that much without being told which.
        let hidden = ServerMsg::Deck {
            rev: 1,
            current: 0,
            slides: slides.clone(),
        }
        .redacted()
        .unwrap();
        let ServerMsg::Deck { slides, .. } = hidden else {
            panic!("not a deck");
        };
        let q = slides[0].question.as_ref().unwrap();
        assert!(q.multi, "the room was not told it may pick several");
        assert!(q.correct.is_empty(), "the room was told which");
    }

    #[test]
    fn a_vote_saved_before_multi_select_still_loads() {
        // The old file shape: one bare number per voter.
        let saved = vec![crate::persist::PersistedSession {
            id: "abc123".into(),
            owner_token: "tok".into(),
            cohost_token: "co".into(),
            markdown: "# One right\n\n- [ ] a\n- [x] b".into(),
            current: 0,
            rev: 1,
            votes: HashMap::from([(
                0,
                HashMap::from([("sam".to_string(), crate::persist::Choice::One(1))]),
            )]),
            revealed: HashSet::from([0]),
            questions: Vec::new(),
            next_question_id: 1,
            names: HashMap::from([("sam".to_string(), "Sam".to_string())]),
            participants: HashSet::from(["sam".to_string()]),
            idle_seconds: 0,
        }];

        let reg = Registry::new(Duration::from_secs(3600));
        assert_eq!(reg.import(saved), 1);

        let ServerMsg::Scores { items } = reg.scores("abc123").unwrap() else {
            panic!("no scores");
        };
        assert_eq!(
            items[0].score, 1,
            "a vote from before multi select stopped counting"
        );
    }
}
