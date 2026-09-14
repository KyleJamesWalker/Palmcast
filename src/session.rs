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

    /// What a token may do here.
    ///
    /// Constant time both ways, and both comparisons always run, so neither
    /// which token matched nor whether any did can be read off the clock.
    /// Every caller goes through this: a second hand-rolled comparison is a
    /// second chance to get constant time wrong.
    fn role_of(&self, token: &str) -> Role {
        let mc: bool = self.owner_token.as_bytes().ct_eq(token.as_bytes()).into();
        let cohost: bool = self.cohost_token.as_bytes().ct_eq(token.as_bytes()).into();
        match (mc, cohost) {
            (true, _) => Role::Mc,
            (_, true) => Role::CoHost,
            _ => Role::Viewer,
        }
    }

    pub fn snapshot(&self) -> ServerMsg {
        ServerMsg::Deck {
            rev: self.rev,
            current: self.current,
            slides: self.slides.clone(),
        }
    }
    /// Serializes once and hands the same bytes to every socket in the room.
    ///
    /// Called with the registry lock held, so a change and the message that
    /// announces it cannot be split by another writer.
    fn emit(&self, msg: &ServerMsg) {
        let _ = self.tx.send(Frame::new(msg));
    }

    /// Every slide that already holds votes, so a console opened part way
    /// through a round knows where the room stands.
    fn tallies(&self) -> Vec<ServerMsg> {
        let mut slides: Vec<usize> = self.votes.keys().copied().collect();
        slides.sort_unstable();
        slides
            .into_iter()
            .filter_map(|slide| {
                let (counts, total) = self.counts(slide);
                (total > 0).then_some(ServerMsg::Tally {
                    slide,
                    counts,
                    total,
                })
            })
            .collect()
    }

    /// Every answer the presenter has already opened. Unlike a tally this is
    /// for everyone, because the point of a reveal is that the answer is now
    /// public.
    fn reveals(&self) -> Vec<ServerMsg> {
        let mut slides: Vec<usize> = self.revealed.iter().copied().collect();
        slides.sort_unstable();
        slides
            .into_iter()
            .filter_map(|slide| {
                let correct = self
                    .slides
                    .get(slide)
                    .and_then(|s| s.question.as_ref())
                    .map(|q| q.correct.clone())?;
                let (counts, total) = self.counts(slide);
                Some(ServerMsg::Reveal {
                    slide,
                    correct,
                    counts,
                    total,
                })
            })
            .collect()
    }

    /// Everything a socket needs to be correct: on arrival, and again after it
    /// has fallen behind and missed messages.
    ///
    /// One list rather than one per caller, so the two paths cannot drift, and
    /// one lock rather than five, so a socket cannot be hydrated from a mix of
    /// states that never existed together. Tallies are staff only, because the
    /// room seeing the split is the thing a tally is withheld for.
    pub fn catch_up(&self, is_staff: bool) -> Vec<ServerMsg> {
        let mut out = vec![self.snapshot(), self.question_list(), self.score_table()];
        out.extend(self.reveals());
        if is_staff {
            out.extend(self.tallies());
        }
        out
    }

    /// `None` when the caller does not drive or the index is out of range.
    pub fn goto(&mut self, token: &str, index: usize) -> Option<ServerMsg> {
        if !self.role_of(token).drives() || index >= self.slides.len() {
            return None;
        }
        self.current = index;
        self.touched = Instant::now();
        let msg = ServerMsg::Move { current: index };
        self.emit(&msg);
        Some(msg)
    }

    pub fn replace_deck(
        &mut self,
        role: Role,
        base_rev: Option<u64>,
        markdown: &str,
    ) -> Result<ServerMsg, EditError> {
        if !role.edits() {
            return Err(EditError::Forbidden);
        }
        // Two people can be editing at once. Whoever saves second is told,
        // rather than quietly writing over the first.
        if let Some(base) = base_rev
            && base != self.rev
        {
            return Err(EditError::Stale { current: self.rev });
        }
        self.markdown = markdown.to_string();
        let rebuilt = deck::parse(markdown);
        // A typo fixed on slide one must not throw away a quiz in progress, so
        // only the questions whose options actually changed lose their votes.
        let intact: HashSet<usize> = rebuilt
            .iter()
            .enumerate()
            .filter(|(index, slide)| {
                match (
                    self.slides.get(*index).and_then(|s| s.question.as_ref()),
                    slide.question.as_ref(),
                ) {
                    (Some(before), Some(after)) => before.options == after.options,
                    _ => false,
                }
            })
            .map(|(index, _)| index)
            .collect();
        self.votes.retain(|slide, _| intact.contains(slide));
        self.revealed.retain(|slide| intact.contains(slide));
        self.slides = rebuilt;
        self.rev += 1;
        self.current = self.current.min(self.slides.len() - 1);
        self.touched = Instant::now();
        let msg = self.snapshot();
        self.emit(&msg);
        Ok(msg)
    }

    /// `None` when the room is full, which the caller turns into a closed
    /// socket rather than a silent viewer who sees nothing.
    pub fn join(&mut self) -> Option<ServerMsg> {
        if self.viewers >= MAX_VIEWERS {
            return None;
        }
        self.viewers += 1;
        self.touched = Instant::now();
        let msg = ServerMsg::Viewers {
            count: self.viewers,
        };
        self.emit(&msg);
        Some(msg)
    }

    pub fn leave(&mut self) -> ServerMsg {
        self.viewers = self.viewers.saturating_sub(1);
        let msg = ServerMsg::Viewers {
            count: self.viewers,
        };
        self.emit(&msg);
        msg
    }

    /// Records one vote and returns the tally. A voter who answers twice
    /// replaces their own vote rather than adding one.
    pub fn answer(&mut self, slide: usize, who: &str, options: &[usize]) -> Option<ServerMsg> {
        if self.revealed.contains(&slide) {
            return None;
        }
        let question = self.slides.get(slide).and_then(|s| s.question.as_ref())?;
        let width = question.options.len();
        // A single answer question takes one pick however many arrive.
        let limit = if question.multi { width } else { 1 };

        let mut chosen: Vec<usize> = options.iter().copied().filter(|o| *o < width).collect();
        chosen.sort_unstable();
        chosen.dedup();
        if chosen.is_empty() || chosen.len() > limit {
            return None;
        }
        if !self.admit(who) {
            return None;
        }

        self.votes
            .entry(slide)
            .or_default()
            .insert(who.to_string(), chosen);
        self.touched = Instant::now();
        let (counts, total) = self.counts(slide);
        let msg = ServerMsg::Tally {
            slide,
            counts,
            total,
        };
        self.emit(&msg);
        Some(msg)
    }

    pub fn react(&mut self, who: &str, kind: Reaction) -> Option<ServerMsg> {
        let now = Instant::now();
        if !self.admit(who) {
            return None;
        }
        if let Some(last) = self.last_reaction.get(who)
            && now.duration_since(*last) < REACTION_GAP
        {
            return None;
        }
        self.last_reaction.insert(who.to_string(), now);
        self.touched = now;
        let msg = ServerMsg::React { kind };
        self.emit(&msg);
        Some(msg)
    }

    pub fn ask(&mut self, who: &str, text: &str) -> Option<ServerMsg> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_QUESTION_CHARS {
            return None;
        }
        if self.questions.len() >= MAX_QUESTIONS {
            return None;
        }
        let now = Instant::now();
        if !self.admit(who) {
            return None;
        }
        if let Some(last) = self.last_ask.get(who)
            && now.duration_since(*last) < ASK_GAP
        {
            return None;
        }
        self.last_ask.insert(who.to_string(), now);

        let question_id = self.next_question_id;
        self.next_question_id += 1;
        // The asker's own vote, so a question starts at one rather than zero.
        let voters = HashSet::from([who.to_string()]);
        self.questions.push(StoredQuestion {
            id: question_id,
            text: text.to_string(),
            answered: false,
            voters,
        });
        self.touched = now;
        let msg = self.question_list();
        self.emit(&msg);
        Some(msg)
    }

    pub fn upvote(&mut self, who: &str, question: u64) -> Option<ServerMsg> {
        if !self.admit(who) {
            return None;
        }
        let found = self.questions.iter_mut().find(|q| q.id == question)?;
        // A set, so a second tap from the same browser is not a second vote.
        if !found.voters.insert(who.to_string()) {
            return None;
        }
        self.touched = Instant::now();
        let msg = self.question_list();
        self.emit(&msg);
        Some(msg)
    }

    pub fn mark_answered(&mut self, role: Role, question: u64) -> Option<ServerMsg> {
        if !role.edits() {
            return None;
        }
        let found = self.questions.iter_mut().find(|q| q.id == question)?;
        found.answered = true;
        self.touched = Instant::now();
        let msg = self.question_list();
        self.emit(&msg);
        Some(msg)
    }

    pub fn set_name(&mut self, who: &str, name: &str) -> Option<ServerMsg> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return None;
        }
        if !self.admit(who) {
            return None;
        }
        self.names.insert(who.to_string(), name.to_string());
        self.touched = Instant::now();
        let msg = self.score_table();
        self.emit(&msg);
        Some(msg)
    }

    pub fn reveal(&mut self, token: &str, slide: usize) -> Option<ServerMsg> {
        if !self.role_of(token).drives() {
            return None;
        }
        let correct = self
            .slides
            .get(slide)
            .and_then(|s| s.question.as_ref())
            .map(|q| q.correct.clone())?;
        self.revealed.insert(slide);
        self.touched = Instant::now();
        let (counts, total) = self.counts(slide);
        let msg = ServerMsg::Reveal {
            slide,
            correct,
            counts,
            total,
        };
        self.emit(&msg);
        // The board only changes when an answer opens, so it rides along with
        // the reveal rather than on a timer, and under the same lock.
        self.emit(&self.score_table());
        Some(msg)
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

    /// Runs `act` against one room while the map is locked.
    ///
    /// The rules live on `Session`, so this is the only way in and the lock is
    /// held for the whole of a change and the message announcing it. `None`
    /// means no such room, which is a different answer from a rule refusing.
    pub fn with<T>(&self, id: &str, act: impl FnOnce(&Session) -> T) -> Option<T> {
        self.lock().get(id).map(act)
    }

    pub fn with_mut<T>(&self, id: &str, act: impl FnOnce(&mut Session) -> T) -> Option<T> {
        self.lock().get_mut(id).map(act)
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

    pub fn markdown(&self, id: &str) -> Option<String> {
        self.with(id, |s| s.markdown.clone())
    }

    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<Arc<Frame>>> {
        self.with(id, |s| s.tx.subscribe())
    }

    /// A room nobody has is a room nobody drives, so an unknown id is a viewer.
    pub fn role(&self, id: &str, token: &str) -> Role {
        self.with(id, |s| s.role_of(token)).unwrap_or(Role::Viewer)
    }

    pub fn cohost_token(&self, id: &str, token: &str) -> Option<String> {
        self.with(id, |s| {
            s.role_of(token).drives().then(|| s.cohost_token.clone())
        })
        .flatten()
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

    /// A tally may never go backwards.
    ///
    /// Every vote is applied under the lock and then broadcast under a second
    /// one. Two voters landing together can have the later tally sent first,
    /// leaving the room showing a count the room has already passed.
    #[test]
    fn concurrent_votes_are_broadcast_in_the_order_they_were_applied() {
        use std::sync::Arc as StdArc;

        let reg = registry();
        let (id, _) = reg.create("# q\n\n- [ ] a\n- [x] b").unwrap();
        let mut rx = reg.subscribe(&id).unwrap();

        let voters = 64;
        let reg = StdArc::new(reg);
        let gate = StdArc::new(std::sync::Barrier::new(voters));
        let hands: Vec<_> = (0..voters)
            .map(|n| {
                let reg = StdArc::clone(&reg);
                let gate = StdArc::clone(&gate);
                let id = id.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    reg.with_mut(&id, |s| s.answer(0, &format!("who{n}"), &[0]));
                })
            })
            .collect();
        for hand in hands {
            hand.join().unwrap();
        }

        let mut high = 0usize;
        let mut totals = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&frame.owner).unwrap();
            if value["type"] == "tally" {
                totals.push(value["total"].as_u64().unwrap() as usize);
            }
        }
        for (nth, total) in totals.iter().enumerate() {
            assert!(
                *total >= high,
                "tally {nth} went backwards: {total} after {high} (order {totals:?})"
            );
            high = *total;
        }
        assert_eq!(
            totals.last().copied(),
            Some(voters),
            "the room ended on a stale count"
        );
    }

    #[test]
    fn a_participant_cap_bounds_a_room() {
        let reg = registry();
        let (id, _) = reg.create("# q\n\n- [ ] a\n- [x] b").unwrap();

        for n in 0..MAX_PARTICIPANTS {
            assert!(
                reg.with_mut(&id, |s| s.answer(0, &format!("who{n}"), &[0]))
                    .flatten()
                    .is_some(),
                "voter {n} inside the cap was turned away"
            );
        }
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "one-too-many", &[0]))
                .flatten()
                .is_none(),
            "a fresh id past the cap still voted"
        );
        // Someone already admitted keeps taking part.
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "who0", &[1]))
                .flatten()
                .is_some()
        );
    }

    #[test]
    fn the_cap_does_not_grow_the_maps_past_it() {
        let reg = registry();
        let (id, _) = reg.create("# hi").unwrap();
        for n in 0..(MAX_PARTICIPANTS + 50) {
            let _ = reg
                .with_mut(&id, |s| s.react(&format!("who{n}"), Reaction::Clap))
                .flatten();
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
        before
            .with_mut(&id, |s| s.set_name("sam", "Sam"))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.set_name("alex", "Alex"))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.answer(0, "sam", &[1]))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.answer(0, "alex", &[0]))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.reveal(&mc, 0))
            .flatten()
            .unwrap();
        // A vote on a question the mc has not opened yet.
        before
            .with_mut(&id, |s| s.answer(1, "sam", &[0]))
            .flatten()
            .unwrap();

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // The opened answer is still open.
        let reveals = after.with(&id, Session::reveals).unwrap();
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
        let tallies = after.with(&id, Session::tallies).unwrap();
        assert_eq!(tallies.len(), 2, "a slide with votes lost its tally");

        // Scores recompute from the votes, so only the opened question counts.
        let ServerMsg::Scores { items } = after.with(&id, Session::score_table).unwrap() else {
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
        before
            .with_mut(&id, |s| s.set_name("sam", "Sam"))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.answer(0, "sam", &[1]))
            .flatten()
            .unwrap();

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // The same mc token still opens it, and the vote from before counts.
        after
            .with_mut(&id, |s| s.reveal(&mc, 0))
            .flatten()
            .expect("the mc token stopped working");
        let ServerMsg::Scores { items } = after.with(&id, Session::score_table).unwrap() else {
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
            .with_mut(&id, |s| s.replace_deck(Role::Mc, Some(1), "# Second"))
            .expect("the room is gone")
            .expect("the first edit should land");

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());

        // Somebody who opened the deck before that edit still has revision 1.
        assert!(
            matches!(
                after.with_mut(&id, |s| s.replace_deck(Role::Mc, Some(1), "# Stale")),
                Some(Err(EditError::Stale { current: 2 }))
            ),
            "a stale save was accepted after a restart"
        );
        // And somebody current still saves.
        assert!(
            after
                .with_mut(&id, |s| s.replace_deck(Role::Mc, Some(2), "# Current"))
                .is_some_and(|saved| saved.is_ok())
        );
        let _ = mc;
    }

    #[test]
    fn replacing_the_deck_leaves_the_questions_alone() {
        // Question ids are their own sequence, unrelated to slide positions, so
        // an edit has no business touching them.
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create("# One\n\n---\n\n# Two").unwrap();
        reg.with_mut(&id, |s| s.ask("sam", "Why not Go?"))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.ask("alex", "How fast is it?"))
            .flatten()
            .unwrap();

        let ServerMsg::Questions { items } = reg.with(&id, Session::question_list).unwrap() else {
            panic!("no questions");
        };
        let ids: Vec<u64> = items.iter().map(|q| q.id).collect();

        reg.with_mut(&id, |s| {
            s.replace_deck(Role::Mc, None, "# Rewritten entirely")
        })
        .unwrap_or(Err(EditError::Gone))
        .unwrap();

        let ServerMsg::Questions { items } = reg.with(&id, Session::question_list).unwrap() else {
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
        assert!(
            before
                .with_mut(&id, |s| s.react("sam", Reaction::Clap))
                .flatten()
                .is_some()
        );
        // Immediately again is too soon.
        assert!(
            before
                .with_mut(&id, |s| s.react("sam", Reaction::Clap))
                .flatten()
                .is_none()
        );
        // Somebody else is unaffected.
        assert!(
            before
                .with_mut(&id, |s| s.react("alex", Reaction::Clap))
                .flatten()
                .is_some()
        );

        let after = Registry::new(Duration::from_secs(3600));
        after.import(before.export());
        // A restart is not a punishment: the gap does not survive it.
        assert!(
            after
                .with_mut(&id, |s| s.react("sam", Reaction::Clap))
                .flatten()
                .is_some(),
            "a restart left somebody unable to react"
        );
    }

    const MULTI: &str = "# Which tracks?\n\n- [x] AI\n- [x] Data Engineering\n- [x] Architecture\n- [ ] Quantum Game Boy";

    #[test]
    fn a_multi_answer_question_scores_only_the_whole_set() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, mc) = reg.create(MULTI).unwrap();
        for who in ["ada", "bo", "cy", "di"] {
            reg.with_mut(&id, |s| s.set_name(who, who))
                .flatten()
                .unwrap();
        }

        reg.with_mut(&id, |s| s.answer(0, "ada", &[0, 1, 2]))
            .flatten()
            .unwrap(); // exactly right
        reg.with_mut(&id, |s| s.answer(0, "bo", &[0]))
            .flatten()
            .unwrap(); // one of three
        reg.with_mut(&id, |s| s.answer(0, "cy", &[0, 1, 2, 3]))
            .flatten()
            .unwrap(); // all four
        reg.with_mut(&id, |s| s.answer(0, "di", &[3]))
            .flatten()
            .unwrap(); // the joke answer
        reg.with_mut(&id, |s| s.reveal(&mc, 0)).flatten().unwrap();

        let ServerMsg::Scores { items } = reg.with(&id, Session::score_table).unwrap() else {
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
        reg.with_mut(&id, |s| s.set_name("ada", "Ada"))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.answer(0, "ada", &[2, 0, 1]))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.reveal(&mc, 0)).flatten().unwrap();

        let ServerMsg::Scores { items } = reg.with(&id, Session::score_table).unwrap() else {
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
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "ada", &[1]))
                .flatten()
                .is_some()
        );
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "bo", &[0, 1]))
                .flatten()
                .is_none(),
            "two picks landed on a single answer question"
        );
    }

    #[test]
    fn a_tally_counts_people_once_however_many_they_pick() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create(MULTI).unwrap();
        reg.with_mut(&id, |s| s.answer(0, "ada", &[0, 1, 2]))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.answer(0, "bo", &[0]))
            .flatten()
            .unwrap();

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
            reg.with_mut(&id, |s| s.answer(0, "ada", &[]))
                .flatten()
                .is_none(),
            "an empty pick landed"
        );
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "ada", &[9]))
                .flatten()
                .is_none(),
            "a pick past the options landed"
        );
        // A repeated option is one option, not two.
        assert!(
            reg.with_mut(&id, |s| s.answer(0, "ada", &[1, 1, 1]))
                .flatten()
                .is_some()
        );
        let map = reg.lock();
        assert_eq!(map.get(&id).unwrap().votes[&0]["ada"], vec![1]);
    }

    #[test]
    fn a_question_says_whether_several_answers_are_right() {
        let reg = Registry::new(Duration::from_secs(3600));
        let (id, _mc) = reg.create(MULTI).unwrap();
        let ServerMsg::Deck { slides, .. } = reg.with(&id, Session::snapshot).unwrap() else {
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

        let ServerMsg::Scores { items } = reg.with("abc123", Session::score_table).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(
            items[0].score, 1,
            "a vote from before multi select stopped counting"
        );
    }
}
