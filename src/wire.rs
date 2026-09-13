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
}

impl ServerMsg {
    /// Speaker notes never leave the presenter's socket.
    pub fn redacted(&self) -> ServerMsg {
        match self {
            ServerMsg::Deck {
                rev,
                current,
                slides,
            } => ServerMsg::Deck {
                rev: *rev,
                current: *current,
                slides: slides
                    .iter()
                    .map(|s| Slide {
                        html: s.html.clone(),
                        notes: String::new(),
                        // The answer is the whole game, so it stays server side
                        // until the presenter reveals it.
                        question: s.question.as_ref().map(|q| Question {
                            options: q.options.clone(),
                            correct: Vec::new(),
                        }),
                    })
                    .collect(),
            },
            other => other.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Goto { index: usize },
}
