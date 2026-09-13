use serde::{Deserialize, Serialize};

use crate::deck::{Question, Slide};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Deck {
        rev: u64,
        current: usize,
        slides: Vec<Slide>,
    },
    Move {
        current: usize,
    },
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
                slides,
            } => Some(ServerMsg::Deck {
                rev: *rev,
                current: *current,
                slides: slides
                    .iter()
                    .map(|s| Slide {
                        html: s.html.clone(),
                        notes: String::new(),
                        question: s.question.as_ref().map(|q| Question {
                            options: q.options.clone(),
                            correct: Vec::new(),
                        }),
                    })
                    .collect(),
            }),
            ServerMsg::Tally { .. } => None,
            other => Some(other.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Goto { index: usize },
    Answer { slide: usize, option: usize },
    Reveal { slide: usize },
}
