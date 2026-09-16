use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::deck::{Look, Question, Slide};

/// Why a socket was closed on arrival, or shortly after.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Full,
    Locked,
    Removed,
}

/// A closed set, so nothing a viewer types ever reaches another viewer's
/// markup. The view picks the glyph from the variant.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    Clap,
    Laugh,
    Think,
    Love,
    Wow,
}

/// One row of the running order. The deck stays behind: only a title, who is
/// giving it, and how long it runs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineupEntry {
    pub id: u64,
    pub title: String,
    pub by: String,
    pub slides: usize,
}

/// One talk, whole. Answered to a request carrying either the host's token or
/// the talk's own, never broadcast: the note the host left is between those
/// two and nobody else.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TalkDetail {
    pub id: u64,
    pub title: String,
    pub markdown: String,
    pub by: String,
    /// Where it sits in the running order, counting from one. `None` once the
    /// host has taken it off.
    pub position: Option<usize>,
    pub staged: bool,
    pub dropped: bool,
    /// Why the host took it off, when they said. Empty when they did not.
    pub note: String,
}

/// A question from the floor. `text` is whatever a viewer typed, so every view
/// puts it on screen as text and never as markup.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudienceQuestion {
    pub id: u64,
    pub text: String,
    pub votes: usize,
    pub answered: bool,
}

/// One line of the leaderboard. `name` is whatever a viewer typed, so every
/// view puts it on screen as text and never as markup.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScoreRow {
    pub name: String,
    pub score: usize,
    /// The browser id behind the name. Host only, so the host can remove
    /// somebody from the room. Nobody else has a use for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// The answer to a `ping`. Identical for every audience, so `redacted`
    /// passes it through on the default arm.
    Pong,
    Deck {
        rev: u64,
        current: usize,
        /// How much of the current slide has come in. Zero is the slide as it
        /// first lands, before any of its staged items.
        step: usize,
        /// The theme the deck asked for, with any knobs it turned. `None`
        /// leaves the view on its own default, which is what a deck that named
        /// nothing wants.
        theme: Option<Look>,
        slides: Vec<Slide>,
    },
    Move {
        current: usize,
        step: usize,
    },
    /// How many phones are in the room. Presenter only: it is drawn on the
    /// console and nowhere else, and a room of four hundred does not need four
    /// hundred copies of its own size.
    Viewers {
        count: usize,
    },
    /// Live vote counts while a question is open. Presenter only, because the
    /// room seeing the split as it forms changes how the room votes.
    Tally {
        slide: usize,
        counts: Vec<usize>,
        total: usize,
    },
    /// The leaderboard, best first. Only people who set a name appear, so
    /// naming yourself is how you opt into being scored.
    Scores {
        items: Vec<ScoreRow>,
    },
    /// The whole question list, most wanted first. Sent whole rather than as a
    /// delta: the list is small and a resync beats a merge bug on a phone that
    /// slept through three updates.
    Questions {
        items: Vec<AudienceQuestion>,
    },
    /// Someone in the room reacting. Everyone sees it, because a room that can
    /// see itself react is the point.
    React {
        kind: Reaction,
    },
    /// The lineup of submitted talks. Everyone sees who is up, because a room
    /// that can see the running order is the point of an open mic. The markdown
    /// never travels with it: an unstaged talk is the speaker's own until the
    /// host puts it on.
    Lineup {
        items: Vec<LineupEntry>,
        /// Talks the host has taken off the running order. Staff only, because
        /// a room does not need to watch what was pulled from it.
        dropped: Vec<LineupEntry>,
        /// The talk currently on stage, if any.
        staged: Option<u64>,
        open: bool,
        /// How long the deck that is up has been up, so every console shows the
        /// same speaker clock whatever its own clock says.
        elapsed_ms: u64,
    },
    /// The clock on the current slide, or none. Sent as time left rather than
    /// a deadline, so a phone with its clock wrong still counts down with the
    /// room.
    Timer {
        slide: Option<usize>,
        remaining_ms: u64,
    },
    /// Whether the room is taking new phones.
    Lock {
        on: bool,
    },
    /// One socket, told why it is being closed, and then closed. Never
    /// broadcast.
    Refused {
        reason: Refusal,
    },
    /// The host has removed somebody. Presenter only as a message, and the
    /// frame itself makes every socket check whether it was the one removed.
    Removed {
        who: String,
    },
    /// Who drives now. Sent to everyone so a speaker's own phone can show the
    /// controls the moment the host hands over, without asking.
    Baton {
        /// The talk whose owner drives, or None while the host does.
        talk: Option<u64>,
    },
    /// Every screen in the room showing the way in, so somebody who arrived
    /// late can join off the phone next to them. Everyone sees it, including
    /// the stage screen, which is the one the whole room is already facing.
    Qr {
        on: bool,
    },
    /// The presenter opening the answer to everyone.
    Reveal {
        slide: usize,
        correct: Vec<usize>,
        counts: Vec<usize>,
        total: usize,
    },
}

impl ServerMsg {
    /// What a socket that is not the presenter may see. `None` means the
    /// message is not theirs at all.
    pub fn redacted(&self) -> Option<ServerMsg> {
        match self {
            ServerMsg::Deck {
                rev,
                current,
                step,
                theme,
                slides,
            } => Some(ServerMsg::Deck {
                rev: *rev,
                current: *current,
                step: *step,
                theme: theme.clone(),
                slides: slides
                    .iter()
                    .map(|s| Slide {
                        html: s.html.clone(),
                        notes: String::new(),
                        steps: s.steps,
                        transition: s.transition.clone(),
                        // The look paints the audience's own screen, so it has
                        // to reach them. It names a stylesheet and nothing else.
                        theme: s.theme.clone(),
                        timer: s.timer,
                        question: s.question.as_ref().map(|q| Question {
                            options: q.options.clone(),
                            multi: q.multi,
                            correct: Vec::new(),
                        }),
                    })
                    .collect(),
            }),
            ServerMsg::Lineup {
                items,
                staged,
                open,
                elapsed_ms,
                ..
            } => Some(ServerMsg::Lineup {
                items: items.clone(),
                dropped: Vec::new(),
                staged: *staged,
                open: *open,
                elapsed_ms: *elapsed_ms,
            }),
            ServerMsg::Scores { items } => Some(ServerMsg::Scores {
                items: items
                    .iter()
                    .map(|row| ScoreRow {
                        name: row.name.clone(),
                        score: row.score,
                        who: None,
                    })
                    .collect(),
            }),
            ServerMsg::Tally { .. } | ServerMsg::Viewers { .. } | ServerMsg::Removed { .. } => None,
            other => Some(other.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// A liveness probe the page can send itself. Browsers do not expose
    /// protocol level pings to JavaScript, so this is how a tab that just woke
    /// finds out whether its socket survived the sleep.
    Ping,
    /// The first frame a presenter sends. Browsers cannot set a header on a
    /// WebSocket, so the token travels here rather than in the URL, where a
    /// proxy would log it. An audience socket sends it empty.
    Auth {
        #[serde(default)]
        token: String,
    },
    Goto {
        index: usize,
        /// How much of that slide to show. Absent means the whole of it, which
        /// is what a jump to another slide means.
        #[serde(default)]
        step: usize,
    },
    /// The host putting a talk on stage, or clearing the stage with None.
    Stage {
        talk: Option<u64>,
    },
    /// The host handing the controls over, or taking them back with None.
    Hand {
        talk: Option<u64>,
    },
    /// The host opening or closing submissions.
    Submissions {
        open: bool,
    },
    /// The host moving a talk to another place in the running order, counting
    /// from zero.
    Reorder {
        talk: u64,
        index: usize,
    },
    /// The host taking a talk off the running order, with whatever they want
    /// its speaker to know. The talk is kept, so the speaker can fix it and put
    /// it back.
    Drop {
        talk: u64,
        #[serde(default)]
        note: String,
    },
    /// The host putting a dropped talk back where it was.
    Restore {
        talk: u64,
    },
    /// The host throwing a talk away for good.
    Remove {
        talk: u64,
    },
    /// The host closing the room to phones it has not seen, or opening it.
    Lock {
        on: bool,
    },
    /// The host removing somebody from the room for good.
    Kick {
        who: String,
    },
    /// The whole selection, replacing whatever this voter chose before.
    Answer {
        slide: usize,
        options: Vec<usize>,
    },
    Reveal {
        slide: usize,
    },
    /// The presenter putting the way in on every screen, or taking it off.
    Qr {
        on: bool,
    },
    React {
        kind: Reaction,
    },
    Ask {
        text: String,
    },
    Upvote {
        question: u64,
    },
    Answered {
        question: u64,
    },
    SetName {
        name: String,
    },
}

/// One broadcast, serialized once per audience so a full room does not pay per
/// socket.
#[derive(Debug)]
pub struct Frame {
    pub owner: String,
    /// `None` when the message is for the presenter alone.
    pub audience: Option<String>,
    /// A handover changes what a socket is allowed to see, and a removal
    /// changes whether it may stay, so a socket that sees this frame go past
    /// re-checks itself. Cheap because it is rare.
    pub rerole: bool,
}

impl Frame {
    pub fn new(msg: &ServerMsg) -> Arc<Frame> {
        let owner = serde_json::to_string(msg).unwrap_or_default();
        let audience = match msg.redacted() {
            // Identical payloads are the common case, so do not serialize twice.
            Some(redacted)
                if matches!(
                    msg,
                    ServerMsg::Deck { .. } | ServerMsg::Lineup { .. } | ServerMsg::Scores { .. }
                ) =>
            {
                Some(serde_json::to_string(&redacted).unwrap_or_default())
            }
            Some(_) => Some(owner.clone()),
            None => None,
        };
        Arc::new(Frame {
            owner,
            audience,
            rerole: matches!(msg, ServerMsg::Baton { .. } | ServerMsg::Removed { .. }),
        })
    }

    pub fn for_socket(&self, is_owner: bool) -> Option<&str> {
        if is_owner {
            Some(&self.owner)
        } else {
            self.audience.as_deref()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::deck::Slide;

    fn deck_msg() -> ServerMsg {
        ServerMsg::Deck {
            rev: 1,
            current: 0,
            step: 0,
            theme: None,
            slides: vec![Slide {
                html: "<h1>Hi</h1>".into(),
                notes: "the secret note".into(),
                steps: 0,
                transition: None,
                theme: Some(Look {
                    name: "neon".into(),
                    knobs: BTreeMap::from([("heading".into(), "#ff8800".into())]),
                }),
                timer: Some(30_000),
                question: Some(Question {
                    options: vec!["a".into(), "b".into()],
                    multi: false,
                    correct: vec![1],
                }),
            }],
        }
    }

    #[test]
    fn a_deck_frame_holds_a_separate_redacted_copy() {
        let frame = Frame::new(&deck_msg());
        let owner = frame.for_socket(true).unwrap();
        let audience = frame.for_socket(false).unwrap();

        assert!(owner.contains("the secret note"));
        assert!(!audience.contains("the secret note"));
        assert!(audience.contains("\"correct\":[]"), "{audience}");
        assert!(owner.contains("\"correct\":[1]"), "{owner}");
    }

    #[test]
    fn an_identical_message_is_not_serialized_twice() {
        let frame = Frame::new(&ServerMsg::Move {
            current: 2,
            step: 0,
        });
        assert_eq!(frame.for_socket(true), frame.for_socket(false));
    }

    #[test]
    fn the_room_is_not_told_what_was_pulled_from_the_running_order() {
        let entry = LineupEntry {
            id: 1,
            title: "Borrow checking".into(),
            by: "Ada".into(),
            slides: 3,
        };
        let frame = Frame::new(&ServerMsg::Lineup {
            items: vec![entry.clone()],
            dropped: vec![LineupEntry {
                title: "Needs a rewrite".into(),
                ..entry
            }],
            staged: None,
            open: true,
            elapsed_ms: 0,
        });

        let owner = frame.for_socket(true).unwrap();
        let audience = frame.for_socket(false).unwrap();
        assert!(owner.contains("Needs a rewrite"));
        assert!(!audience.contains("Needs a rewrite"), "{audience}");
        assert!(audience.contains("Borrow checking"));
    }

    #[test]
    fn a_slide_look_reaches_the_room_that_has_to_draw_it() {
        let Some(ServerMsg::Deck { slides, .. }) = deck_msg().redacted() else {
            panic!("the deck was withheld from the room");
        };
        let look = slides[0].theme.as_ref().expect("the look was withheld");
        assert_eq!(
            look.name, "neon",
            "the audience was not told which look to paint the slide in"
        );
        assert_eq!(
            look.knobs.get("heading").map(String::as_str),
            Some("#ff8800"),
            "the knob the deck turned did not reach the screen it paints"
        );
        // It names a stylesheet the instance already serves to anyone, so there
        // is nothing in it to withhold. The notes beside it still go.
        assert_eq!(slides[0].notes, "");
    }

    #[test]
    fn the_room_is_not_told_how_many_are_watching() {
        let msg = ServerMsg::Viewers { count: 400 };
        assert!(
            msg.redacted().is_none(),
            "the room was sent its own size, once per phone that arrived"
        );
    }

    #[test]
    fn a_timer_reaches_the_room_that_counts_down_with_it() {
        let Some(ServerMsg::Deck { slides, .. }) = deck_msg().redacted() else {
            panic!("the deck was withheld from the room");
        };
        assert_eq!(slides[0].timer, Some(30_000));
    }

    #[test]
    fn the_room_is_not_told_who_is_behind_a_name() {
        let frame = Frame::new(&ServerMsg::Scores {
            items: vec![ScoreRow {
                name: "Ada".into(),
                score: 3,
                who: Some("browser-id-7".into()),
            }],
        });
        let owner = frame.for_socket(true).unwrap();
        let audience = frame.for_socket(false).unwrap();
        assert!(owner.contains("browser-id-7"));
        assert!(!audience.contains("browser-id-7"), "{audience}");
        assert!(!audience.contains("\"who\""), "{audience}");
        assert!(audience.contains("Ada"));
    }

    #[test]
    fn a_removal_makes_every_socket_check_itself_and_tells_the_room_nothing() {
        let frame = Frame::new(&ServerMsg::Removed { who: "x".into() });
        assert!(frame.rerole);
        assert_eq!(frame.for_socket(false), None);
    }

    #[test]
    fn a_presenter_only_message_has_no_audience_copy() {
        let frame = Frame::new(&ServerMsg::Tally {
            slide: 0,
            counts: vec![1, 2],
            total: 3,
        });
        assert!(frame.for_socket(true).is_some());
        assert_eq!(frame.for_socket(false), None, "the tally leaked");
    }
}
