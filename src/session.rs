use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rand::Rng;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;

use crate::deck::{self, Slide};
use crate::images::Stored;
use crate::persist::{Choice, PersistedCue, PersistedQuestion, PersistedSession, PersistedTalk};
use crate::wire::{
    AudienceQuestion, Frame, LineupEntry, Reaction, ScoreRow, ServerMsg, TalkDetail,
};

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
/// Rooms one instance holds at once, unless `--max-sessions` says otherwise.
/// Each holds a deck, its votes and any pictures, so the ceiling is memory.
pub const DEFAULT_MAX_SESSIONS: usize = 500;
/// How far a socket may fall behind before it is resynced instead. Each slot is
/// an Arc<Frame>, so the room pays a pointer per slot and not a deck.
const CHANNEL_DEPTH: usize = 256;
/// A room bigger than this is not a bar, and every socket costs a broadcast
/// receiver.
const MAX_VIEWERS: usize = 400;
/// A name is a label on a leaderboard, not a field for prose.
const MAX_NAME_CHARS: usize = 24;
/// Bounds one broadcast. Nobody reads past the top of a leaderboard anyway.
const MAX_SCORE_ROWS: usize = 50;
const TOKEN_LEN: usize = 32;
/// An evening of lightning talks is a dozen, not a thousand. Every entry holds
/// a whole deck, so this is what bounds a room that takes submissions.
const MAX_TALKS: usize = 40;
/// One person cannot fill the running order on their own.
const MAX_TALKS_PER_PERSON: usize = 3;
/// A lightning talk, not a keynote. Also what one submission can cost the room.
const MAX_TALK_BYTES: usize = 64 * 1024;
const MAX_TITLE_CHARS: usize = 60;
/// A note is a line telling a speaker what to fix, the same length the room
/// gets for a question.
const MAX_NOTE_CHARS: usize = 280;
/// An evening of slides, not a gallery. Every picture sits in memory for the
/// life of the room and is served to everyone in it.
const MAX_IMAGES: usize = 40;
const MAX_IMAGE_BYTES: usize = 24 * 1024 * 1024;
/// One picture at a time from any one phone.
const UPLOAD_GAP: Duration = Duration::from_secs(2);

/// What a socket or a request is allowed to do.
///
/// One person drives, so the room never watches two people fight over the
/// slide. A co-host works on the deck while that happens, which is the point:
/// writing the next question from the floor without taking the room with you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Viewer,
    /// Whoever the host has handed the controls to. Drives the room and nothing
    /// else: a speaker moves their own slides without gaining the lineup, the
    /// other talks, or the ability to take the stage back.
    Driver,
    CoHost,
    Mc,
}

impl Role {
    pub fn drives(self) -> bool {
        matches!(self, Role::Mc | Role::Driver)
    }

    pub fn edits(self) -> bool {
        matches!(self, Role::Mc | Role::CoHost)
    }

    /// Who runs the evening. The host keeps this whatever they hand out, so
    /// taking the controls back is always theirs to do.
    pub fn hosts(self) -> bool {
        self == Role::Mc
    }
}

/// A deck somebody submitted to the running order.
pub struct Talk {
    pub id: u64,
    pub title: String,
    pub markdown: String,
    /// Minted at submission and kept in the submitter's browser. It drives only
    /// while the host has handed over, so a leaked one is worth what the host
    /// allows and no more.
    pub token: String,
    pub by: String,
    /// Set when the host takes it off the running order, holding whatever they
    /// wanted the speaker to know. The talk is kept rather than deleted, so the
    /// speaker can fix what was wrong and put it back.
    pub dropped: Option<String>,
}

/// The host's own deck, parked while a talk is on stage.
pub struct Parked {
    pub markdown: String,
    pub slides: Vec<Slide>,
    pub current: usize,
    /// Held rather than banked, so the deck coming back scores once and not
    /// twice. `score_table` counts these while they are parked.
    pub votes: HashMap<usize, HashMap<String, Vec<usize>>>,
    pub revealed: HashSet<usize>,
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
    /// How much of the current slide the room has been shown. A slide holding
    /// no staged items has one step, zero, and never leaves it.
    pub step: usize,
    pub viewers: usize,
    /// Set when the count moved, cleared when the room has been told. The
    /// telling is on a timer, so a QR scan is one frame rather than hundreds.
    pub viewers_dirty: bool,
    pub touched: Instant,
    pub tx: broadcast::Sender<Arc<Frame>>,
    /// slide index -> voter id -> the options they chose. One selection each,
    /// and a later one replaces it rather than adding to it.
    pub votes: HashMap<usize, HashMap<String, Vec<usize>>>,
    pub revealed: HashSet<usize>,
    pub last_reaction: HashMap<String, Instant>,
    pub questions: Vec<StoredQuestion>,
    pub last_ask: HashMap<String, Instant>,
    /// Pictures the room is holding. They live and die with the session, so a
    /// swept room takes its images with it.
    pub images: Vec<Stored>,
    pub image_bytes: usize,
    pub last_upload: HashMap<String, Instant>,
    pub next_question_id: u64,
    pub participants: HashSet<String>,
    /// participant id -> the name they chose.
    pub names: HashMap<String, String>,
    pub lineup: Vec<Talk>,
    pub next_talk_id: u64,
    /// The talk on stage. `None` means the host's own deck is live.
    pub staged: Option<u64>,
    /// The host's deck while a talk stands in front of it.
    pub parked: Option<Parked>,
    /// The talk whose owner drives. `None` means the host drives.
    pub baton: Option<u64>,
    pub submissions_open: bool,
    /// Whether every screen is showing the way into the room. Deliberately not
    /// persisted: a room coming back from a restart should come back on its
    /// slides, not on a QR nobody is standing in front of any more.
    pub qr_open: bool,
    /// Points from talks that have already come down.
    ///
    /// A score is derived from the votes so a late reveal or a correction can
    /// never leave a stale total behind. That holds inside one deck and cannot
    /// survive the next one replacing it, so what a talk was worth is banked
    /// when it ends. Keyed by browser id, because a name can change.
    pub banked: HashMap<String, usize>,
    /// What was on screen and when, for anyone cutting a recording afterwards.
    pub timeline: Vec<Cue>,
    pub opened: SystemTime,
}

/// One moment the room saw something new.
pub struct Cue {
    pub at: SystemTime,
    pub talk: Option<u64>,
    pub slide: usize,
    pub title: String,
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

    /// One point for each revealed question this person got right on the deck
    /// that is up. Derived from the votes rather than counted as they arrive,
    /// so a late reveal or a correction cannot leave a stale total behind.
    fn earned(&self, who: &str) -> usize {
        Self::points_on(&self.slides, &self.revealed, &self.votes, who)
    }

    /// `earned` against any deck, so a parked one still counts toward the board.
    fn points_on(
        slides: &[Slide],
        revealed: &HashSet<usize>,
        votes: &HashMap<usize, HashMap<String, Vec<usize>>>,
        who: &str,
    ) -> usize {
        revealed
            .iter()
            .filter(|slide| {
                let Some(question) = slides.get(**slide).and_then(|s| s.question.as_ref()) else {
                    return false;
                };
                // The point is for the answer, not for one part of it, so the
                // selection has to be exactly right.
                votes
                    .get(*slide)
                    .and_then(|cast| cast.get(who))
                    .is_some_and(|chosen| {
                        let mut want = question.correct.clone();
                        want.sort_unstable();
                        *chosen == want
                    })
            })
            .count()
    }

    /// What the parked host deck is still worth to this person.
    fn parked_points(&self, who: &str) -> usize {
        self.parked
            .as_ref()
            .map(|p| Self::points_on(&p.slides, &p.revealed, &p.votes, who))
            .unwrap_or(0)
    }

    /// The evening's total: what earlier talks were worth, plus the deck that
    /// is up.
    fn score_table(&self) -> ServerMsg {
        let mut items: Vec<ScoreRow> = self
            .names
            .iter()
            .map(|(who, name)| ScoreRow {
                name: name.clone(),
                score: self.banked.get(who).copied().unwrap_or(0)
                    + self.earned(who)
                    + self.parked_points(who),
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
    pub fn role_of(&self, token: &str) -> Role {
        let mc: bool = self.owner_token.as_bytes().ct_eq(token.as_bytes()).into();
        let cohost: bool = self.cohost_token.as_bytes().ct_eq(token.as_bytes()).into();
        // Only the talk actually holding the baton is checked, so a speaker
        // whose turn has passed drives nothing with the same token.
        let driver: bool = self
            .baton
            .and_then(|id| self.lineup.iter().find(|talk| talk.id == id))
            .map(|talk| talk.token.as_bytes().ct_eq(token.as_bytes()).into())
            .unwrap_or(false);
        match (mc, cohost, driver) {
            (true, _, _) => Role::Mc,
            (_, true, _) => Role::CoHost,
            (_, _, true) => Role::Driver,
            _ => Role::Viewer,
        }
    }

    /// Whether a driver may see notes, answers and tallies.
    ///
    /// The baton is independent of the stage, so the host can hand the controls
    /// to a speaker while the host deck is still up. A driver reads staff state
    /// only for their own talk, never for whatever else is on screen.
    pub fn driver_sees_staff_view(&self, token: &str) -> bool {
        self.role_of(token) == Role::Driver && self.staged == self.baton
    }

    pub fn snapshot(&self) -> ServerMsg {
        ServerMsg::Deck {
            rev: self.rev,
            current: self.current,
            step: self.step,
            // Read off the live markdown rather than held as a field, so a
            // staged talk brings its own theme without a second thing to keep
            // in step with `self.markdown`.
            theme: deck::theme_of(&self.markdown),
            slides: self.slides.clone(),
        }
    }

    /// How many presses the slide at `index` takes before the next slide.
    fn steps_at(&self, index: usize) -> usize {
        self.slides.get(index).map(|s| s.steps).unwrap_or(0)
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
        let mut out = vec![
            self.snapshot(),
            self.question_list(),
            self.score_table(),
            self.lineup_msg(),
            self.baton_msg(),
            ServerMsg::Qr { on: self.qr_open },
        ];
        out.extend(self.reveals());
        if is_staff {
            out.extend(self.tallies());
        }
        out
    }

    /// `None` when the caller does not drive or the index is out of range.
    ///
    /// A step past the end of the slide is clamped rather than refused. The
    /// console sends the position it can see, and a deck edited under it can
    /// leave that one item further on than the slide now goes.
    pub fn goto(&mut self, token: &str, index: usize, step: usize) -> Option<ServerMsg> {
        if !self.role_of(token).drives() || index >= self.slides.len() {
            return None;
        }
        self.current = index;
        self.step = step.min(self.steps_at(index));
        self.touched = Instant::now();
        // Moving the deck says the sharing is over. Without this a presenter
        // who flips the room to a QR and carries on talking leaves the room
        // reading a QR code instead of the slides.
        self.close_qr();
        self.mark();
        let msg = ServerMsg::Move {
            current: index,
            step: self.step,
        };
        self.emit(&msg);
        Some(msg)
    }

    /// Puts the way into the room on every screen, or takes it off.
    ///
    /// Whoever drives, which is the host or a speaker holding the controls:
    /// the person standing in front of the room is the one who knows somebody
    /// just walked in.
    pub fn show_qr(&mut self, token: &str, on: bool) -> Option<ServerMsg> {
        if !self.role_of(token).drives() || self.qr_open == on {
            return None;
        }
        self.qr_open = on;
        self.touched = Instant::now();
        let msg = ServerMsg::Qr { on };
        self.emit(&msg);
        Some(msg)
    }

    /// Takes the QR down if it was up, telling the room. Silent when it was
    /// already down, so an ordinary slide change costs no frame.
    fn close_qr(&mut self) {
        if !self.qr_open {
            return;
        }
        self.qr_open = false;
        self.emit(&ServerMsg::Qr { on: false });
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
        self.step = self.step.min(self.steps_at(self.current));
        self.touched = Instant::now();
        let msg = self.snapshot();
        self.emit(&msg);
        Ok(msg)
    }

    /// `None` when the room is full, which the caller turns into a closed
    /// socket rather than a silent viewer who sees nothing.
    /// Counts a phone in. The room is not told here: a QR code going up puts
    /// hundreds of phones through this in a few seconds, and one frame per
    /// arrival to every socket is a burst nobody reads. `flush_viewers` sends
    /// the total a moment later instead.
    pub fn join(&mut self) -> Option<ServerMsg> {
        if self.viewers >= MAX_VIEWERS {
            return None;
        }
        self.viewers += 1;
        self.touched = Instant::now();
        self.viewers_dirty = true;
        Some(ServerMsg::Viewers {
            count: self.viewers,
        })
    }

    pub fn leave(&mut self) -> ServerMsg {
        self.viewers = self.viewers.saturating_sub(1);
        self.viewers_dirty = true;
        ServerMsg::Viewers {
            count: self.viewers,
        }
    }

    /// Sends the count the room has reached, if it moved since the last one.
    fn flush_viewers(&mut self) -> bool {
        if !self.viewers_dirty {
            return false;
        }
        self.viewers_dirty = false;
        self.emit(&ServerMsg::Viewers {
            count: self.viewers,
        });
        true
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
        // Only what is still open counts against the cap, so a long evening of
        // questions asked and answered does not close the floor.
        if self.questions.iter().filter(|q| !q.answered).count() >= MAX_QUESTIONS {
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
        // The list itself stays bounded, so an evening cannot grow it without
        // limit. The oldest answered question is the one nobody is waiting on.
        while self.questions.len() >= MAX_QUESTIONS
            && let Some(oldest) = self.questions.iter().position(|q| q.answered)
        {
            self.questions.remove(oldest);
        }
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
    /// The leaderboard as rows, for anything that is not a wire message.
    pub fn board(&self) -> Vec<ScoreRow> {
        match self.score_table() {
            ServerMsg::Scores { items } => items,
            _ => Vec::new(),
        }
    }

    fn lineup_msg(&self) -> ServerMsg {
        let rows = |dropped: bool| -> Vec<LineupEntry> {
            self.lineup
                .iter()
                .filter(|talk| talk.dropped.is_some() == dropped)
                .map(|talk| LineupEntry {
                    id: talk.id,
                    title: talk.title.clone(),
                    by: talk.by.clone(),
                    slides: deck::parse(&talk.markdown).len(),
                })
                .collect()
        };
        ServerMsg::Lineup {
            items: rows(false),
            dropped: rows(true),
            staged: self.staged,
            open: self.submissions_open,
        }
    }

    /// The talks that are actually in the running order, in order, as indexes
    /// into the lineup. A dropped talk keeps its slot so putting it back puts
    /// it back where it was, which means a position is never a lineup index.
    fn running(&self) -> Vec<usize> {
        self.lineup
            .iter()
            .enumerate()
            .filter(|(_, talk)| talk.dropped.is_none())
            .map(|(index, _)| index)
            .collect()
    }

    fn baton_msg(&self) -> ServerMsg {
        ServerMsg::Baton { talk: self.baton }
    }

    /// Notes what the room is looking at, so a recording can be cut against it
    /// afterwards. Only a change is worth a cue: holding on a slide is one
    /// moment, however long it lasts.
    fn mark(&mut self) {
        let title = self
            .staged
            .and_then(|id| self.lineup.iter().find(|t| t.id == id))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Host".to_string());
        if let Some(last) = self.timeline.last()
            && last.talk == self.staged
            && last.slide == self.current
        {
            return;
        }
        self.timeline.push(Cue {
            at: SystemTime::now(),
            talk: self.staged,
            slide: self.current,
            title,
        });
    }

    /// Adds a talk to the running order and returns the token that will drive
    /// it. `None` when the room is not taking submissions or the caps say no.
    pub fn submit(&mut self, who: &str, title: &str, markdown: &str) -> Option<(u64, String)> {
        if !self.submissions_open
            || self.lineup.len() >= MAX_TALKS
            || markdown.len() > MAX_TALK_BYTES
        {
            return None;
        }
        if !self.admit(who) {
            return None;
        }
        let by = self.names.get(who).cloned().unwrap_or_default();
        if !by.is_empty()
            && self.lineup.iter().filter(|t| t.by == by).count() >= MAX_TALKS_PER_PERSON
        {
            return None;
        }
        let title: String = match title.trim() {
            "" => first_heading(markdown),
            given => given.chars().take(MAX_TITLE_CHARS).collect(),
        };
        let id = self.next_talk_id;
        self.next_talk_id += 1;
        let token = random_string(TOKEN_LEN);
        self.lineup.push(Talk {
            id,
            title,
            markdown: markdown.to_string(),
            token: token.clone(),
            by,
            dropped: None,
        });
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        Some((id, token))
    }

    /// One talk, whole, for whoever is entitled to it: the host reading it
    /// before putting it up, or the speaker checking their own.
    /// Keeps a picture and returns the id to reach it by.
    ///
    /// `None` when the room is holding as much as it will, or when this phone
    /// asked again too soon. The bytes have already been shrunk by the caller,
    /// so what is counted here is what the room will actually serve.
    pub fn store_image(&mut self, who: &str, bytes: Vec<u8>, kind: &'static str) -> Option<String> {
        if !self.admit(who) {
            return None;
        }
        let now = Instant::now();
        if let Some(last) = self.last_upload.get(who)
            && now.duration_since(*last) < UPLOAD_GAP
        {
            return None;
        }
        if self.images.len() >= MAX_IMAGES || self.image_bytes + bytes.len() > MAX_IMAGE_BYTES {
            return None;
        }
        self.last_upload.insert(who.to_string(), now);
        let id = random_string(16);
        self.image_bytes += bytes.len();
        self.images.push(Stored {
            id: id.clone(),
            kind,
            bytes,
        });
        self.touched = Instant::now();
        Some(id)
    }

    /// Whether the room is open to the floor. A room taking talks is taking
    /// the pictures those talks need.
    pub fn takes_talks(&self) -> bool {
        self.submissions_open
    }

    pub fn image(&self, id: &str) -> Option<&Stored> {
        self.images.iter().find(|held| held.id == id)
    }

    pub fn talk_detail(&self, talk: u64) -> Option<TalkDetail> {
        let held = self.lineup.iter().find(|t| t.id == talk)?;
        let position = self
            .running()
            .iter()
            .position(|slot| self.lineup[*slot].id == talk)
            .map(|at| at + 1);
        Some(TalkDetail {
            id: held.id,
            title: held.title.clone(),
            // A staged talk is being edited live, so the live deck is the truth.
            markdown: if self.staged == Some(talk) {
                self.markdown.clone()
            } else {
                held.markdown.clone()
            },
            by: held.by.clone(),
            position,
            staged: self.staged == Some(talk),
            dropped: held.dropped.is_some(),
            note: held.dropped.clone().unwrap_or_default(),
        })
    }

    /// True when this is the token minted for that talk. Constant time, like
    /// every other token comparison here.
    pub fn owns_talk(&self, talk: u64, token: &str) -> bool {
        self.lineup
            .iter()
            .find(|t| t.id == talk)
            .map(|t| t.token.as_bytes().ct_eq(token.as_bytes()).into())
            .unwrap_or(false)
    }

    /// Replaces a talk the room has not seen yet.
    ///
    /// A talk still waiting is the speaker's to rewrite, and one the host
    /// dropped is theirs to fix, which puts it back where it was. The talk on
    /// stage is the exception: that deck is the room's, and it is edited
    /// through the console like any other live deck.
    pub fn update_talk(&mut self, talk: u64, title: &str, markdown: &str) -> Result<(), TalkError> {
        if markdown.len() > MAX_TALK_BYTES {
            return Err(TalkError::TooLarge);
        }
        if self.staged == Some(talk) {
            return Err(TalkError::Staged);
        }
        let Some(held) = self.lineup.iter_mut().find(|t| t.id == talk) else {
            return Err(TalkError::Gone);
        };
        held.title = match title.trim() {
            "" => first_heading(markdown),
            given => given.chars().take(MAX_TITLE_CHARS).collect(),
        };
        held.markdown = markdown.to_string();
        held.dropped = None;
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        Ok(())
    }

    pub fn set_submissions(&mut self, role: Role, open: bool) -> bool {
        if !role.hosts() {
            return false;
        }
        self.submissions_open = open;
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        true
    }

    /// Takes a talk off the running order, with whatever the host wants its
    /// speaker to know.
    ///
    /// The deck is kept rather than deleted, because whoever wrote it is
    /// standing in the room: a talk pulled for running twice too long is one
    /// edit away from being usable. A staged talk comes off the stage first, so
    /// the room is never left looking at a deck the running order has lost.
    pub fn drop_talk(&mut self, role: Role, talk: u64, note: &str) -> bool {
        if !role.hosts() || !self.lineup.iter().any(|t| t.id == talk) {
            return false;
        }
        self.clear_stage_of(role, talk);
        let note: String = note.trim().chars().take(MAX_NOTE_CHARS).collect();
        if let Some(held) = self.lineup.iter_mut().find(|t| t.id == talk) {
            held.dropped = Some(note);
        }
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        true
    }

    /// Puts a dropped talk back where it was.
    pub fn restore_talk(&mut self, role: Role, talk: u64) -> bool {
        if !role.hosts() {
            return false;
        }
        let Some(held) = self.lineup.iter_mut().find(|t| t.id == talk) else {
            return false;
        };
        if held.dropped.take().is_none() {
            return false;
        }
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        true
    }

    /// Throws a talk away. Nothing comes back from this, which is why dropping
    /// one is the gentler thing the host reaches for first.
    pub fn remove_talk(&mut self, role: Role, talk: u64) -> bool {
        if !role.hosts() || !self.lineup.iter().any(|t| t.id == talk) {
            return false;
        }
        self.clear_stage_of(role, talk);
        self.lineup.retain(|t| t.id != talk);
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        true
    }

    /// Gets a talk off the stage and out of the driving seat, for the two ways
    /// a talk stops being part of the evening.
    fn clear_stage_of(&mut self, role: Role, talk: u64) {
        if self.staged == Some(talk) {
            self.stage(role, None);
        }
        if self.baton == Some(talk) {
            self.baton = None;
            self.emit(&self.baton_msg());
        }
    }

    /// Moves a talk to another place in the running order, counting from zero.
    ///
    /// The order talks arrive in is the order somebody typed fastest, which is
    /// nobody's idea of an evening. A position counts the running order alone,
    /// so a dropped talk sitting between two of them changes nothing.
    pub fn reorder(&mut self, role: Role, talk: u64, index: usize) -> bool {
        if !role.hosts() {
            return false;
        }
        let running = self.running();
        let Some(from) = running
            .iter()
            .position(|slot| self.lineup[*slot].id == talk)
        else {
            return false;
        };
        let to = index.min(running.len().saturating_sub(1));
        if to == from {
            return false;
        }
        let moved = self.lineup.remove(running[from]);
        // Read again, because the removal shifted everything behind it.
        let landing = self.running().get(to).copied().unwrap_or(self.lineup.len());
        self.lineup.insert(landing, moved);
        self.touched = Instant::now();
        self.emit(&self.lineup_msg());
        true
    }

    /// Puts a talk in front of the room, or clears the stage with `None`.
    ///
    /// The speaker takes the controls with it: that is what selecting a talk
    /// means. The host can still take them back, which is `hand`.
    ///
    /// Questions belong to the talk they were asked during, so the floor clears
    /// here. The leaderboard does not: it runs the whole evening.
    pub fn stage(&mut self, role: Role, talk: Option<u64>) -> bool {
        if !role.hosts() || self.staged == talk {
            return false;
        }
        if let Some(id) = talk
            && !self
                .lineup
                .iter()
                .any(|t| t.id == id && t.dropped.is_none())
        {
            return false;
        }

        // Whatever is live now goes back where it came from, so an edit made
        // while a talk was up is the version that gets exported.
        match self.staged {
            Some(id) => {
                // What the talk that just ended was worth, before the votes
                // behind it go. A talk never comes back to its votes, so this
                // is the only chance to keep them.
                let owed: Vec<(String, usize)> = self
                    .votes
                    .values()
                    .flat_map(|cast| cast.keys())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .map(|who| (who.clone(), self.earned(who)))
                    .collect();
                for (who, points) in owed {
                    *self.banked.entry(who).or_insert(0) += points;
                }
                if let Some(held) = self.lineup.iter_mut().find(|t| t.id == id) {
                    held.markdown = self.markdown.clone();
                }
            }
            None => {
                self.parked = Some(Parked {
                    markdown: self.markdown.clone(),
                    slides: self.slides.clone(),
                    current: self.current,
                    votes: std::mem::take(&mut self.votes),
                    revealed: std::mem::take(&mut self.revealed),
                });
            }
        }

        let returning = match talk {
            Some(id) => {
                let markdown = self
                    .lineup
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.markdown.clone())
                    .unwrap_or_default();
                self.slides = deck::parse(&markdown);
                self.markdown = markdown;
                self.current = 0;
                None
            }
            None => {
                let back = self.parked.take().unwrap_or_else(|| Parked {
                    markdown: self.markdown.clone(),
                    slides: self.slides.clone(),
                    current: self.current,
                    votes: HashMap::new(),
                    revealed: HashSet::new(),
                });
                self.markdown = back.markdown;
                self.slides = back.slides;
                self.current = back.current.min(self.slides.len().saturating_sub(1));
                Some((back.votes, back.revealed))
            }
        };

        self.staged = talk;
        self.baton = talk;
        self.step = 0;
        self.rev += 1;
        // The floor clears with the deck. The host deck's own votes come back
        // with it, so a replayed round cannot score a second time.
        self.questions.clear();
        self.last_ask.clear();
        let (votes, revealed) = returning.unwrap_or_default();
        self.votes = votes;
        self.revealed = revealed;
        self.touched = Instant::now();
        self.mark();

        self.emit(&self.snapshot());
        self.emit(&self.lineup_msg());
        self.emit(&self.baton_msg());
        self.emit(&self.question_list());
        true
    }

    /// Hands the controls to a talk's owner, or takes them back with `None`.
    /// Independent of the stage, so a co-presenter can drive someone else's
    /// deck and the host can take over without changing what is on screen.
    pub fn hand(&mut self, role: Role, talk: Option<u64>) -> bool {
        if !role.hosts() {
            return false;
        }
        if let Some(id) = talk
            && !self
                .lineup
                .iter()
                .any(|t| t.id == id && t.dropped.is_none())
        {
            return false;
        }
        self.baton = talk;
        self.touched = Instant::now();
        self.emit(&self.baton_msg());
        true
    }
}

/// The first heading in a deck, which is what a speaker has already written
/// rather than a second thing to ask them for.
fn first_heading(markdown: &str) -> String {
    markdown
        .lines()
        .find_map(|line| {
            let text = line.trim_start_matches('#').trim();
            line.trim_start()
                .starts_with('#')
                .then(|| text.chars().take(MAX_TITLE_CHARS).collect::<String>())
                .filter(|t: &String| !t.is_empty())
        })
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Why a speaker's own edit to their own talk did not land.
#[derive(Debug, PartialEq, Eq)]
pub enum TalkError {
    /// The room is looking at it. That deck is edited through the console.
    Staged,
    TooLarge,
    Gone,
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
    max_sessions: usize,
}

impl Registry {
    pub fn new(ttl: Duration) -> Self {
        Self::with_limit(ttl, DEFAULT_MAX_SESSIONS)
    }

    pub fn with_limit(ttl: Duration, max_sessions: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            max_sessions,
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

    /// `None` when the instance is already holding all the rooms it will.
    pub fn create(&self, markdown: &str) -> Option<(String, String)> {
        let mut map = self.lock();
        if map.len() >= self.max_sessions {
            // Drop anything idle before turning a real room away.
            let ttl = self.ttl;
            map.retain(|_, s| s.viewers > 0 || s.touched.elapsed() < ttl);
            if map.len() >= self.max_sessions {
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
        let (tx, _) = broadcast::channel(CHANNEL_DEPTH);
        map.insert(
            id.clone(),
            Session {
                owner_token: token.clone(),
                cohost_token: cohost.clone(),
                markdown: markdown.to_string(),
                slides: deck::parse(markdown),
                rev: 1,
                current: 0,
                step: 0,
                viewers: 0,
                viewers_dirty: false,
                touched: Instant::now(),
                tx,
                votes: HashMap::new(),
                revealed: HashSet::new(),
                last_reaction: HashMap::new(),
                questions: Vec::new(),
                last_ask: HashMap::new(),
                images: Vec::new(),
                image_bytes: 0,
                last_upload: HashMap::new(),
                next_question_id: 1,
                participants: HashSet::new(),
                names: HashMap::new(),
                lineup: Vec::new(),
                next_talk_id: 1,
                staged: None,
                parked: None,
                baton: None,
                submissions_open: false,
                qr_open: false,
                banked: HashMap::new(),
                timeline: Vec::new(),
                opened: SystemTime::now(),
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

    /// Tells every room whose count moved what it moved to. Called on a timer,
    /// so a QR code going up costs one frame per room rather than one per phone
    /// per socket. Returns how many rooms had something to say.
    pub fn flush_viewers(&self) -> usize {
        let mut told = 0;
        for session in self.lock().values_mut() {
            if session.flush_viewers() {
                told += 1;
            }
        }
        told
    }

    /// What a socket holding this token may see, under one lock.
    pub fn staff(&self, id: &str, token: &str) -> bool {
        self.with(id, |s| {
            s.role_of(token).edits() || s.driver_sees_staff_view(token)
        })
        .unwrap_or(false)
    }

    pub fn cohost_token(&self, id: &str, token: &str) -> Option<String> {
        self.with(id, |s| {
            s.role_of(token).hosts().then(|| s.cohost_token.clone())
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
                step: s.step,
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
                lineup: s
                    .lineup
                    .iter()
                    .map(|talk| PersistedTalk {
                        id: talk.id,
                        title: talk.title.clone(),
                        markdown: talk.markdown.clone(),
                        token: talk.token.clone(),
                        by: talk.by.clone(),
                        dropped: talk.dropped.is_some(),
                        note: talk.dropped.clone().unwrap_or_default(),
                    })
                    .collect(),
                next_talk_id: s.next_talk_id,
                staged: s.staged,
                baton: s.baton,
                submissions_open: s.submissions_open,
                parked: s.parked.as_ref().map(|p| p.markdown.clone()),
                parked_current: s.parked.as_ref().map(|p| p.current).unwrap_or(0),
                parked_votes: s
                    .parked
                    .as_ref()
                    .map(|p| {
                        p.votes
                            .iter()
                            .map(|(slide, cast)| {
                                let cast = cast
                                    .iter()
                                    .map(|(who, chosen)| {
                                        (who.clone(), Choice::from(chosen.clone()))
                                    })
                                    .collect();
                                (*slide, cast)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                parked_revealed: s
                    .parked
                    .as_ref()
                    .map(|p| p.revealed.clone())
                    .unwrap_or_default(),
                banked: s.banked.clone(),
                opened_ms: millis(s.opened),
                timeline: s
                    .timeline
                    .iter()
                    .map(|cue| PersistedCue {
                        at_ms: millis(cue.at),
                        talk: cue.talk,
                        slide: cue.slide,
                        title: cue.title.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Returns how many came back. Viewer counts start at zero, because nobody
    /// is connected to a process that has just started.
    pub fn import(&self, saved: Vec<PersistedSession>) -> usize {
        let mut map = self.lock();
        let mut restored = 0;
        let now = Instant::now();
        for item in saved.into_iter().take(self.max_sessions) {
            // Reparsed here, not restored, so a change to the parser can give
            // the same markdown a different shape. Anything held against a slide
            // position has to be checked against the deck that actually came
            // back, or it describes a slide that is no longer there.
            let slides = deck::parse(&item.markdown);
            let current = item.current.min(slides.len().saturating_sub(1));
            let step = item
                .step
                .min(slides.get(current).map(|s| s.steps).unwrap_or(0));
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
            let (tx, _) = broadcast::channel(CHANNEL_DEPTH);
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
                    step,
                    viewers: 0,
                    viewers_dirty: false,
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
                    // Pictures are not in the state file, so a restored room
                    // comes back without them.
                    images: Vec::new(),
                    image_bytes: 0,
                    last_upload: HashMap::new(),
                    next_question_id: item.next_question_id.max(1),
                    participants: item.participants,
                    names: item.names,
                    lineup: item
                        .lineup
                        .into_iter()
                        .map(|talk| Talk {
                            id: talk.id,
                            title: talk.title,
                            markdown: talk.markdown,
                            token: talk.token,
                            by: talk.by,
                            dropped: talk.dropped.then_some(talk.note),
                        })
                        .collect(),
                    next_talk_id: item.next_talk_id.max(1),
                    staged: item.staged,
                    parked: item.parked.map(|markdown| {
                        let slides = deck::parse(&markdown);
                        let current = item.parked_current.min(slides.len().saturating_sub(1));
                        Parked {
                            markdown,
                            slides,
                            current,
                            votes: item
                                .parked_votes
                                .into_iter()
                                .map(|(slide, cast)| {
                                    let cast = cast
                                        .into_iter()
                                        .map(|(who, chosen)| (who, chosen.into_vec()))
                                        .collect();
                                    (slide, cast)
                                })
                                .collect(),
                            revealed: item.parked_revealed,
                        }
                    }),
                    baton: item.baton,
                    submissions_open: item.submissions_open,
                    // Never restored: a room coming back should come back on
                    // its slides.
                    qr_open: false,
                    banked: item.banked,
                    timeline: item
                        .timeline
                        .into_iter()
                        .map(|cue| Cue {
                            at: from_millis(cue.at_ms),
                            talk: cue.talk,
                            slide: cue.slide,
                            title: cue.title,
                        })
                        .collect(),
                    // A file written before the timeline existed has no start,
                    // and now is the only honest answer for one.
                    opened: match item.opened_ms {
                        0 => SystemTime::now(),
                        ms => from_millis(ms),
                    },
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

fn millis(at: SystemTime) -> u64 {
    at.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn from_millis(ms: u64) -> SystemTime {
    std::time::UNIX_EPOCH + Duration::from_millis(ms)
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

    fn open_room() -> (Registry, String, String) {
        let reg = registry();
        let (id, mc) = reg.create("# Welcome\n\n---\n\n# Up next").unwrap();
        reg.with_mut(&id, |s| s.set_submissions(Role::Mc, true));
        (reg, id, mc)
    }

    fn submit(reg: &Registry, id: &str, who: &str, markdown: &str) -> (u64, String) {
        reg.with_mut(id, |s| s.submit(who, "", markdown))
            .flatten()
            .expect("the submission was refused")
    }

    #[test]
    fn a_closed_room_takes_no_talks() {
        let reg = registry();
        let (id, _) = reg.create("# Welcome").unwrap();
        assert!(
            reg.with_mut(&id, |s| s.submit("who", "", "# Mine"))
                .flatten()
                .is_none(),
            "a room that never opened took a talk"
        );
    }

    #[test]
    fn only_the_host_opens_submissions() {
        let reg = registry();
        let (id, _) = reg.create("# Welcome").unwrap();
        for role in [Role::Viewer, Role::CoHost, Role::Driver] {
            assert!(
                !reg.with_mut(&id, |s| s.set_submissions(role, true))
                    .unwrap()
            );
        }
        assert!(
            reg.with_mut(&id, |s| s.set_submissions(Role::Mc, true))
                .unwrap()
        );
    }

    #[test]
    fn a_title_falls_back_to_the_first_heading() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "\n\n## Borrow checking\n\nbody");
        let ServerMsg::Lineup { items, .. } = reg.with(&id, Session::lineup_msg).unwrap() else {
            panic!("no lineup");
        };
        assert_eq!(items[0].id, talk);
        assert_eq!(items[0].title, "Borrow checking");
    }

    #[test]
    fn the_lineup_never_carries_the_decks() {
        let (reg, id, _) = open_room();
        submit(&reg, &id, "ada", "# Mine\n\nsecret punchline");
        let msg = reg.with(&id, Session::lineup_msg).unwrap();
        let wire = serde_json::to_string(&msg).unwrap();
        assert!(
            !wire.contains("punchline"),
            "an unstaged deck went out to the room: {wire}"
        );
    }

    #[test]
    fn staging_a_talk_puts_it_on_screen_and_parks_the_host_deck() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two\n\n---\n\n# Three");

        assert!(
            reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
                .unwrap()
        );
        let ServerMsg::Deck {
            slides, current, ..
        } = reg.with(&id, Session::snapshot).unwrap()
        else {
            panic!("no deck");
        };
        assert_eq!(slides.len(), 3, "the talk did not go up");
        assert_eq!(current, 0, "a staged talk starts at its first slide");

        // And the host deck comes back untouched.
        assert!(reg.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap());
        let ServerMsg::Deck { slides, .. } = reg.with(&id, Session::snapshot).unwrap() else {
            panic!("no deck");
        };
        assert_eq!(slides.len(), 2, "the host deck did not come back");
    }

    #[test]
    fn staging_clears_the_floor_but_not_the_board() {
        let (reg, id, mc) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n- [x] yes\n- [ ] no");

        // A question and a point earned before the talk goes up.
        reg.with_mut(&id, |s| s.set_name("sam", "Sam"))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.ask("sam", "who is buying"))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        reg.with_mut(&id, |s| s.answer(1, "sam", &[0]))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.reveal(&mc, 1)).flatten().unwrap();

        let ServerMsg::Scores { items } = reg.with(&id, Session::score_table).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(items[0].score, 1);

        // The next talk clears the questions and the votes, and keeps the board.
        let (next, _) = submit(&reg, &id, "bob", "# Bob");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(next)))
            .unwrap();
        let ServerMsg::Questions { items } = reg.with(&id, Session::question_list).unwrap() else {
            panic!("no questions");
        };
        assert!(items.is_empty(), "the floor carried over to the next talk");
        let ServerMsg::Scores { items } = reg.with(&id, Session::score_table).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(items[0].score, 1, "the board reset between talks");
    }

    /// A host deck with a question on its second slide, and submissions open.
    fn open_quiz_room() -> (Registry, String, String) {
        let reg = registry();
        let (id, mc) = reg
            .create("# Welcome\n\n---\n\n- [x] yes\n- [ ] no")
            .unwrap();
        reg.with_mut(&id, |s| s.set_submissions(Role::Mc, true));
        (reg, id, mc)
    }

    fn score_of(reg: &Registry, id: &str, name: &str) -> usize {
        let ServerMsg::Scores { items } = reg.with(id, Session::score_table).unwrap() else {
            panic!("no scores");
        };
        items
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.score)
            .unwrap_or(0)
    }

    /// Answers the host question and reveals it, leaving Sam one point up.
    fn ask_and_reveal(reg: &Registry, id: &str, mc: &str) {
        reg.with_mut(id, |s| s.set_name("sam", "Sam"))
            .flatten()
            .unwrap();
        reg.with_mut(id, |s| s.answer(1, "sam", &[0]))
            .flatten()
            .unwrap();
        reg.with_mut(id, |s| s.reveal(mc, 1)).flatten().unwrap();
    }

    #[test]
    fn the_host_deck_comes_back_with_its_votes_and_reveals() {
        let (reg, id, mc) = open_quiz_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        ask_and_reveal(&reg, &id, &mc);

        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        reg.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();

        let caught = reg.with(&id, |s| s.catch_up(false)).unwrap();
        assert!(
            caught
                .iter()
                .any(|m| matches!(m, ServerMsg::Reveal { slide: 1, .. })),
            "the reveal was lost while the talk was up"
        );
        assert!(
            reg.with_mut(&id, |s| s.answer(1, "sam", &[1]))
                .flatten()
                .is_none(),
            "a question that came back revealed took another vote"
        );
    }

    #[test]
    fn a_round_replayed_after_a_talk_does_not_score_twice() {
        let (reg, id, mc) = open_quiz_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        ask_and_reveal(&reg, &id, &mc);
        assert_eq!(score_of(&reg, &id, "Sam"), 1);

        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        assert_eq!(
            score_of(&reg, &id, "Sam"),
            1,
            "the parked deck's point left the board"
        );
        reg.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();

        reg.with_mut(&id, |s| s.reveal(&mc, 1));
        assert_eq!(
            score_of(&reg, &id, "Sam"),
            1,
            "the replayed round banked a second point"
        );
    }

    #[test]
    fn parked_quiz_state_survives_a_restart() {
        let (before, id, mc) = open_quiz_room();
        let (talk, _) = submit(&before, &id, "ada", "# Ada");
        ask_and_reveal(&before, &id, &mc);
        before
            .with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        let after = registry();
        assert_eq!(after.import(before.export()), 1);
        after.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();

        let caught = after.with(&id, |s| s.catch_up(false)).unwrap();
        assert!(
            caught
                .iter()
                .any(|m| matches!(m, ServerMsg::Reveal { slide: 1, .. })),
            "the reveal did not survive the restart"
        );
        assert_eq!(
            score_of(&after, &id, "Sam"),
            1,
            "the parked point was lost across the restart"
        );
    }

    #[test]
    fn viewer_changes_are_coalesced_into_one_frame() {
        let reg = registry();
        let (id, _mc) = reg.create("# Welcome").unwrap();
        let mut console = reg.subscribe(&id).unwrap();

        // A QR code going up. Nothing is sent per arrival.
        for _ in 0..50 {
            reg.with_mut(&id, Session::join).flatten().unwrap();
        }
        assert!(
            console.try_recv().is_err(),
            "a phone arriving told the whole room about it"
        );

        assert_eq!(reg.flush_viewers(), 1, "the room was never told at all");
        let frame = console.try_recv().expect("no count after the flush");
        assert!(
            frame.owner.contains("\"count\":50"),
            "the count did not catch up in one frame: {}",
            frame.owner
        );
        assert!(
            frame.audience.is_none(),
            "the room was sent its own size after all"
        );
        assert!(
            console.try_recv().is_err(),
            "the flush sent more than one frame"
        );

        // Nothing moved, so there is nothing to say.
        assert_eq!(reg.flush_viewers(), 0, "a quiet room was told again");
    }

    #[test]
    fn answered_questions_make_room_for_new_ones() {
        let reg = registry();
        let (id, _mc) = reg.create("# Welcome").unwrap();

        // A distinct asker each time, because one asker is rate limited.
        for i in 0..MAX_QUESTIONS {
            assert!(
                reg.with_mut(&id, |s| s.ask(&format!("asker-{i}"), "why"))
                    .flatten()
                    .is_some(),
                "the floor closed at question {i}"
            );
        }
        assert!(
            reg.with_mut(&id, |s| s.ask("one-more", "why"))
                .flatten()
                .is_none(),
            "the cap did not hold"
        );

        // The host deals with one, which frees a place for the next.
        let first = reg
            .with(&id, |s| s.questions.first().map(|q| q.id))
            .flatten()
            .unwrap();
        reg.with_mut(&id, |s| s.mark_answered(Role::Mc, first))
            .flatten()
            .unwrap();
        assert!(
            reg.with_mut(&id, |s| s.ask("one-more", "why"))
                .flatten()
                .is_some(),
            "an answered question still held a place on the floor"
        );

        // And the list stays bounded: the answered one made way rather than
        // the list growing past the cap.
        let (total, open) = reg
            .with(&id, |s| {
                (
                    s.questions.len(),
                    s.questions.iter().filter(|q| !q.answered).count(),
                )
            })
            .unwrap();
        assert_eq!(total, MAX_QUESTIONS, "the question list grew past its cap");
        assert_eq!(open, MAX_QUESTIONS, "the answered question was not reused");
    }

    #[test]
    fn a_driver_sees_staff_state_only_while_their_talk_is_staged() {
        let (reg, id, mc) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada");

        // Handed the controls while the host's own deck is still on screen.
        reg.with_mut(&id, |s| s.hand(Role::Mc, Some(talk))).unwrap();
        assert_eq!(reg.role(&id, &speaker), Role::Driver);
        assert!(
            !reg.staff(&id, &speaker),
            "a driver read the host deck's notes and answers"
        );

        // Their own talk goes up and the notes are theirs to see.
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        assert!(
            reg.staff(&id, &speaker),
            "a speaker lost the notes for their own talk"
        );

        assert!(reg.staff(&id, &mc), "the host stopped being staff");
        assert!(
            !reg.staff(&id, "guessed"),
            "a stranger was treated as staff"
        );
    }

    #[test]
    fn a_speaker_handed_the_controls_cannot_mint_a_cohost_link() {
        let (reg, id, _mc) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada");
        reg.with_mut(&id, |s| s.hand(Role::Mc, Some(talk))).unwrap();

        assert_eq!(reg.role(&id, &speaker), Role::Driver);
        assert!(
            reg.cohost_token(&id, &speaker).is_none(),
            "a driver minted a cohost link and edited their way to the notes"
        );
    }

    #[test]
    fn staging_hands_the_speaker_the_controls_and_the_host_can_take_them_back() {
        let (reg, id, mc) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two");
        assert_eq!(
            reg.role(&id, &speaker),
            Role::Viewer,
            "a talk alone drives nothing"
        );

        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        assert_eq!(reg.role(&id, &speaker), Role::Driver);
        assert!(
            reg.with_mut(&id, |s| s.goto(&speaker, 1, 0))
                .flatten()
                .is_some(),
            "the speaker could not drive their own talk"
        );

        // The host is never not the host.
        assert_eq!(reg.role(&id, &mc), Role::Mc);
        reg.with_mut(&id, |s| s.hand(Role::Mc, None)).unwrap();
        assert_eq!(reg.role(&id, &speaker), Role::Viewer);
        assert!(
            reg.with_mut(&id, |s| s.goto(&speaker, 0, 0))
                .flatten()
                .is_none(),
            "the speaker still drove after the host took over"
        );
        assert!(reg.with_mut(&id, |s| s.goto(&mc, 0, 0)).flatten().is_some());
    }

    #[test]
    fn a_driver_drives_and_nothing_else() {
        let (reg, id, _) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada");
        let (other, _) = submit(&reg, &id, "bob", "# Bob\n\nnot yours");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        let role = reg.role(&id, &speaker);
        assert!(role.drives());
        assert!(!role.edits(), "a speaker gained the deck editor");
        assert!(!role.hosts(), "a speaker gained the running order");
        // So none of the host's controls answer to them.
        assert!(!reg.with_mut(&id, |s| s.stage(role, Some(other))).unwrap());
        assert!(!reg.with_mut(&id, |s| s.hand(role, None)).unwrap());
        assert!(!reg.with_mut(&id, |s| s.drop_talk(role, other, "")).unwrap());
        assert!(
            !reg.with_mut(&id, |s| s.set_submissions(role, false))
                .unwrap()
        );
        assert!(
            reg.with(&id, |s| s.talk_detail(other)).flatten().is_some(),
            "the test is meaningless if the other talk never existed"
        );
    }

    #[test]
    fn the_host_can_hand_the_controls_to_someone_else_mid_talk() {
        let (reg, id, _) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two");
        let (second, helper) = submit(&reg, &id, "bob", "# Bob");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        // A co-presenter drives the deck that is already up.
        reg.with_mut(&id, |s| s.hand(Role::Mc, Some(second)))
            .unwrap();
        assert_eq!(reg.role(&id, &helper), Role::Driver);
        assert_eq!(reg.role(&id, &speaker), Role::Viewer);
        assert!(
            reg.with_mut(&id, |s| s.goto(&helper, 1, 0))
                .flatten()
                .is_some()
        );
        // And the stage did not move.
        let ServerMsg::Lineup { staged, .. } = reg.with(&id, Session::lineup_msg).unwrap() else {
            panic!("no lineup");
        };
        assert_eq!(staged, Some(talk), "handing the controls moved the stage");
    }

    #[test]
    fn an_edit_made_on_stage_is_the_one_that_comes_back() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        reg.with_mut(&id, |s| {
            s.replace_deck(Role::Mc, None, "# Ada\n\n---\n\n# Fixed")
        })
        .unwrap()
        .unwrap();
        reg.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();

        assert_eq!(
            reg.with(&id, |s| s.talk_detail(talk))
                .flatten()
                .unwrap()
                .markdown,
            "# Ada\n\n---\n\n# Fixed",
            "the fix made on stage was lost when the talk came down"
        );
    }

    #[test]
    fn dropping_the_staged_talk_takes_it_off_the_screen_first() {
        let (reg, id, _) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two\n\n---\n\n# Three");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        assert!(
            reg.with_mut(&id, |s| s.drop_talk(Role::Mc, talk, ""))
                .unwrap()
        );

        let ServerMsg::Deck { slides, .. } = reg.with(&id, Session::snapshot).unwrap() else {
            panic!("no deck");
        };
        assert_eq!(slides.len(), 2, "the room was left on a dropped talk");
        assert_eq!(reg.role(&id, &speaker), Role::Viewer);
        let ServerMsg::Lineup { items, staged, .. } = reg.with(&id, Session::lineup_msg).unwrap()
        else {
            panic!("no lineup");
        };
        assert!(items.is_empty());
        assert_eq!(staged, None);
    }

    fn order(reg: &Registry, id: &str) -> Vec<u64> {
        match reg.with(id, Session::lineup_msg).unwrap() {
            ServerMsg::Lineup { items, .. } => items.into_iter().map(|t| t.id).collect(),
            _ => Vec::new(),
        }
    }

    /// Talks arrive in the order somebody typed fastest, which is not an
    /// evening. The host decides what follows what.
    #[test]
    fn the_host_sets_the_running_order() {
        let (reg, id, _) = open_room();
        let (first, _) = submit(&reg, &id, "ada", "# Ada");
        let (second, _) = submit(&reg, &id, "bob", "# Bob");
        let (third, _) = submit(&reg, &id, "cal", "# Cal");

        assert!(
            reg.with_mut(&id, |s| s.reorder(Role::Mc, third, 0))
                .unwrap()
        );
        assert_eq!(order(&reg, &id), vec![third, first, second]);

        assert!(
            reg.with_mut(&id, |s| s.reorder(Role::Mc, third, 2))
                .unwrap()
        );
        assert_eq!(order(&reg, &id), vec![first, second, third]);

        // Past the end is the end rather than a refusal: a phone sends the
        // position it can see.
        assert!(
            reg.with_mut(&id, |s| s.reorder(Role::Mc, first, 99))
                .unwrap()
        );
        assert_eq!(order(&reg, &id), vec![second, third, first]);
    }

    /// A dropped talk keeps its slot in the list it was dropped from, so a
    /// position always counts the running order and never the gaps in it.
    #[test]
    fn a_dropped_talk_does_not_shift_the_order_around_it() {
        let (reg, id, _) = open_room();
        let (first, _) = submit(&reg, &id, "ada", "# Ada");
        let (second, _) = submit(&reg, &id, "bob", "# Bob");
        let (third, _) = submit(&reg, &id, "cal", "# Cal");
        reg.with_mut(&id, |s| s.drop_talk(Role::Mc, second, ""))
            .unwrap();
        assert_eq!(order(&reg, &id), vec![first, third]);

        assert!(
            reg.with_mut(&id, |s| s.reorder(Role::Mc, first, 1))
                .unwrap()
        );
        assert_eq!(order(&reg, &id), vec![third, first]);
        assert!(
            reg.with_mut(&id, |s| s.restore_talk(Role::Mc, second))
                .unwrap()
        );
        assert_eq!(order(&reg, &id).len(), 3);
    }

    #[test]
    fn only_the_host_orders_the_evening() {
        let (reg, id, _) = open_room();
        let (first, speaker) = submit(&reg, &id, "ada", "# Ada");
        let (second, _) = submit(&reg, &id, "bob", "# Bob");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(first)))
            .unwrap();
        let role = reg.role(&id, &speaker);

        assert!(!reg.with_mut(&id, |s| s.reorder(role, second, 0)).unwrap());
        assert!(
            !reg.with_mut(&id, |s| s.reorder(Role::CoHost, second, 0))
                .unwrap()
        );
        assert!(
            !reg.with_mut(&id, |s| s.restore_talk(Role::Viewer, second))
                .unwrap()
        );
        assert_eq!(order(&reg, &id), vec![first, second]);
    }

    /// Whoever wrote the talk is standing in the room, so a drop is a message
    /// and a deck they can still fix, not a deletion.
    #[test]
    fn a_dropped_talk_keeps_its_deck_and_carries_the_note() {
        let (reg, id, _) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two");
        assert!(
            reg.with_mut(&id, |s| s.drop_talk(Role::Mc, talk, "  Twice too long.  "))
                .unwrap()
        );

        assert!(order(&reg, &id).is_empty());
        let detail = reg.with(&id, |s| s.talk_detail(talk)).flatten().unwrap();
        assert!(detail.dropped);
        assert_eq!(detail.note, "Twice too long.");
        assert_eq!(detail.position, None);
        assert_eq!(detail.markdown, "# Ada\n\n---\n\n# Two");

        // And it is nobody's to put on until it comes back.
        assert!(
            !reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
                .unwrap()
        );
        assert!(!reg.with_mut(&id, |s| s.hand(Role::Mc, Some(talk))).unwrap());
        assert_eq!(reg.role(&id, &speaker), Role::Viewer);
    }

    #[test]
    fn a_note_is_a_line_and_not_a_speech() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        reg.with_mut(&id, |s| s.drop_talk(Role::Mc, talk, &"no".repeat(400)))
            .unwrap();
        let detail = reg.with(&id, |s| s.talk_detail(talk)).flatten().unwrap();
        assert_eq!(detail.note.chars().count(), MAX_NOTE_CHARS);
    }

    #[test]
    fn fixing_a_dropped_talk_puts_it_back_where_it_was() {
        let (reg, id, _) = open_room();
        let (first, _) = submit(&reg, &id, "ada", "# Ada");
        let (second, _) = submit(&reg, &id, "bob", "# Bob");
        let (third, _) = submit(&reg, &id, "cal", "# Cal");
        reg.with_mut(&id, |s| s.drop_talk(Role::Mc, second, "Needs an ending"))
            .unwrap();

        reg.with_mut(&id, |s| {
            s.update_talk(second, "", "# Bob\n\n---\n\n# An ending")
        })
        .unwrap()
        .unwrap();

        assert_eq!(order(&reg, &id), vec![first, second, third]);
        let detail = reg.with(&id, |s| s.talk_detail(second)).flatten().unwrap();
        assert!(!detail.dropped);
        assert_eq!(detail.note, "");
        assert_eq!(detail.position, Some(2));
        assert_eq!(detail.title, "Bob", "the retitle did not follow the deck");
    }

    /// The deck the room is looking at belongs to the room. It is edited in the
    /// console, where an edit reaches every phone at once.
    #[test]
    fn a_talk_on_stage_is_not_rewritten_behind_the_room_s_back() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        assert_eq!(
            reg.with_mut(&id, |s| s.update_talk(talk, "", "# Something else"))
                .unwrap(),
            Err(TalkError::Staged)
        );
    }

    #[test]
    fn a_talk_answers_to_its_own_token_and_no_other() {
        let (reg, id, mc) = open_room();
        let (mine, my_token) = submit(&reg, &id, "ada", "# Ada");
        let (_, your_token) = submit(&reg, &id, "bob", "# Bob");

        reg.with(&id, |s| {
            assert!(s.owns_talk(mine, &my_token));
            assert!(
                !s.owns_talk(mine, &your_token),
                "another talk's token opened it"
            );
            assert!(
                !s.owns_talk(mine, &mc),
                "the host token passed as a speaker's"
            );
            assert!(!s.owns_talk(mine, ""), "an empty token opened a talk");
        })
        .unwrap();
    }

    #[test]
    fn removing_a_talk_takes_it_away_for_good() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada");
        reg.with_mut(&id, |s| s.drop_talk(Role::Mc, talk, "not tonight"))
            .unwrap();

        assert!(
            reg.with_mut(&id, |s| s.remove_talk(Role::Mc, talk))
                .unwrap()
        );
        assert!(reg.with(&id, |s| s.talk_detail(talk)).flatten().is_none());
        assert!(
            !reg.with_mut(&id, |s| s.restore_talk(Role::Mc, talk))
                .unwrap()
        );
    }

    #[test]
    fn a_dropped_talk_and_its_note_survive_a_restart() {
        let before = registry();
        let (id, _) = before.create("# Welcome").unwrap();
        before.with_mut(&id, |s| s.set_submissions(Role::Mc, true));
        let (talk, token) = before
            .with_mut(&id, |s| s.submit("ada", "", "# Ada"))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.drop_talk(Role::Mc, talk, "Needs an ending"))
            .unwrap();

        let after = registry();
        assert_eq!(after.import(before.export()), 1);

        let detail = after.with(&id, |s| s.talk_detail(talk)).flatten().unwrap();
        assert!(detail.dropped, "the drop was forgotten across a restart");
        assert_eq!(detail.note, "Needs an ending");
        assert!(order(&after, &id).is_empty());
        assert!(
            after.with(&id, |s| s.owns_talk(talk, &token)).unwrap(),
            "the speaker lost their own talk across a restart"
        );
    }

    #[test]
    fn one_person_cannot_fill_the_running_order() {
        let (reg, id, _) = open_room();
        reg.with_mut(&id, |s| s.set_name("ada", "Ada"))
            .flatten()
            .unwrap();
        for n in 0..MAX_TALKS_PER_PERSON {
            assert!(
                reg.with_mut(&id, |s| s.submit("ada", "", &format!("# Talk {n}")))
                    .flatten()
                    .is_some(),
                "talk {n} inside the cap was refused"
            );
        }
        assert!(
            reg.with_mut(&id, |s| s.submit("ada", "", "# One too many"))
                .flatten()
                .is_none(),
            "one person filled the running order"
        );
        // Somebody else still gets a slot.
        reg.with_mut(&id, |s| s.set_name("bob", "Bob"))
            .flatten()
            .unwrap();
        assert!(
            reg.with_mut(&id, |s| s.submit("bob", "", "# Mine"))
                .flatten()
                .is_some()
        );
    }

    #[test]
    fn the_timeline_records_a_change_and_not_a_pause() {
        let (reg, id, mc) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        reg.with_mut(&id, |s| s.goto(&mc, 1, 0)).flatten().unwrap();
        // Driving to the slide the room is already on is not a new moment.
        reg.with_mut(&id, |s| s.goto(&mc, 1, 0)).flatten().unwrap();

        let cues = reg.with(&id, |s| {
            s.timeline
                .iter()
                .map(|c| (c.talk, c.slide))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            cues.unwrap(),
            vec![(Some(talk), 0), (Some(talk), 1)],
            "the timeline is not what the room actually saw"
        );
    }

    /// A host who restarts mid evening must not lose what the room wrote.
    #[test]
    fn the_running_order_survives_a_restart() {
        let (before, id, _) = open_room();
        before
            .with_mut(&id, |s| s.set_name("ada", "Ada"))
            .flatten()
            .unwrap();
        let (talk, speaker) = submit(&before, &id, "ada", "# Ada\n\n---\n\n# Two");
        submit(&before, &id, "ada", "# Later");
        before
            .with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        let after = registry();
        assert_eq!(after.import(before.export()), 1);

        let ServerMsg::Lineup {
            items,
            staged,
            open,
            ..
        } = after.with(&id, Session::lineup_msg).unwrap()
        else {
            panic!("no lineup");
        };
        assert_eq!(items.len(), 2, "the running order was lost");
        assert_eq!(items[0].by, "Ada");
        assert_eq!(staged, Some(talk), "the room came back on a different deck");
        assert!(open, "the room stopped taking talks across a restart");

        // The speaker keeps driving without being handed a new link.
        assert_eq!(after.role(&id, &speaker), Role::Driver);
        assert!(
            after
                .with_mut(&id, |s| s.goto(&speaker, 1, 0))
                .flatten()
                .is_some()
        );

        // And the host deck is still behind the talk.
        after.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();
        let ServerMsg::Deck { slides, .. } = after.with(&id, Session::snapshot).unwrap() else {
            panic!("no deck");
        };
        assert_eq!(slides.len(), 2, "the parked host deck did not come back");
    }

    #[test]
    fn the_board_survives_a_restart_mid_evening() {
        let (before, id, mc) = open_room();
        let (talk, _) = submit(&before, &id, "ada", "# Ada\n\n---\n\n- [x] yes\n- [ ] no");
        before
            .with_mut(&id, |s| s.set_name("sam", "Sam"))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();
        before
            .with_mut(&id, |s| s.answer(1, "sam", &[0]))
            .flatten()
            .unwrap();
        before
            .with_mut(&id, |s| s.reveal(&mc, 1))
            .flatten()
            .unwrap();
        // Banked as the talk comes down.
        before.with_mut(&id, |s| s.stage(Role::Mc, None)).unwrap();

        let after = registry();
        after.import(before.export());
        let ServerMsg::Scores { items } = after.with(&id, Session::score_table).unwrap() else {
            panic!("no scores");
        };
        assert_eq!(
            items[0].score, 1,
            "points earned before the restart were lost"
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
            ..Default::default()
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
        for _ in 0..(DEFAULT_MAX_SESSIONS + 10) {
            if reg.create("# hi").is_some() {
                made += 1;
            }
        }
        assert_eq!(
            made, DEFAULT_MAX_SESSIONS,
            "the instance created {made} sessions"
        );
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
            ..Default::default()
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
            step: 0,
            theme: None,
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
            ..Default::default()
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

    #[test]
    fn a_deck_that_names_a_theme_sends_it_with_the_slides() {
        let reg = registry();
        let (id, _) = reg.create("<!-- theme: paper -->\n# One").unwrap();
        let ServerMsg::Deck { theme, .. } = reg.with(&id, Session::snapshot).unwrap() else {
            panic!("not a deck");
        };
        assert_eq!(theme.as_deref(), Some("paper"));
    }

    #[test]
    fn a_deck_that_names_no_theme_sends_none() {
        let reg = registry();
        let (id, _) = reg.create("# One").unwrap();
        let ServerMsg::Deck { theme, .. } = reg.with(&id, Session::snapshot).unwrap() else {
            panic!("not a deck");
        };
        assert_eq!(theme, None);
    }

    #[test]
    fn a_staged_talk_brings_its_own_theme_to_the_room() {
        let (reg, id, _) = open_room();
        let (talk, _) = submit(&reg, &id, "ada", "<!-- theme: neon -->\n# My talk");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)))
            .unwrap();

        let ServerMsg::Deck { theme, .. } = reg.with(&id, Session::snapshot).unwrap() else {
            panic!("not a deck");
        };
        assert_eq!(
            theme.as_deref(),
            Some("neon"),
            "the talk's theme did not reach the room"
        );
    }

    fn qr_state(msgs: &[ServerMsg]) -> Option<bool> {
        msgs.iter().find_map(|m| match m {
            ServerMsg::Qr { on } => Some(*on),
            _ => None,
        })
    }

    #[test]
    fn the_presenter_can_put_a_qr_on_every_screen() {
        let (reg, id, mc) = open_room();
        assert!(
            reg.with_mut(&id, |s| s.show_qr(&mc, true))
                .flatten()
                .is_some()
        );
        assert_eq!(
            reg.with(&id, |s| qr_state(&s.catch_up(false))).unwrap(),
            Some(true)
        );
    }

    #[test]
    fn a_phone_joining_is_always_told_whether_the_qr_is_up() {
        let (reg, id, mc) = open_room();
        assert_eq!(
            reg.with(&id, |s| qr_state(&s.catch_up(false))).unwrap(),
            Some(false),
            "a fresh socket was left to guess"
        );
        reg.with_mut(&id, |s| s.show_qr(&mc, true));
        reg.with_mut(&id, |s| s.show_qr(&mc, false));
        assert_eq!(
            reg.with(&id, |s| qr_state(&s.catch_up(false))).unwrap(),
            Some(false),
            "a socket that lagged through the flip would stay stuck on it"
        );
    }

    #[test]
    fn the_room_cannot_put_a_qr_on_its_own_screens() {
        let (reg, id, _) = open_room();
        assert!(
            reg.with_mut(&id, |s| s.show_qr("not-the-token", true))
                .flatten()
                .is_none()
        );
        assert_eq!(
            reg.with(&id, |s| qr_state(&s.catch_up(false))).unwrap(),
            Some(false)
        );
    }

    /// A presenter who flips the room to a QR and then carries on talking has
    /// left the room looking at a QR instead of the slides. Moving the deck is
    /// unambiguous about being done with it.
    #[test]
    fn moving_the_deck_takes_the_qr_back_down() {
        let (reg, id, mc) = open_room();
        reg.with_mut(&id, |s| s.show_qr(&mc, true));
        reg.with_mut(&id, |s| s.goto(&mc, 1, 0));
        assert_eq!(
            reg.with(&id, |s| qr_state(&s.catch_up(false))).unwrap(),
            Some(false)
        );
    }

    #[test]
    fn a_speaker_driving_their_own_talk_can_share_the_room() {
        let (reg, id, _) = open_room();
        let (talk, speaker) = submit(&reg, &id, "ada", "# Ada\n\n---\n\n# Two");
        reg.with_mut(&id, |s| s.stage(Role::Mc, Some(talk)));
        reg.with_mut(&id, |s| s.hand(Role::Mc, Some(talk)));
        assert!(
            reg.with_mut(&id, |s| s.show_qr(&speaker, true))
                .flatten()
                .is_some(),
            "the person in front of the room could not share it"
        );
    }
}
