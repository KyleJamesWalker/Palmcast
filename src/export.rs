//! The evening, in a file you can keep.
//!
//! One zip: every talk as the markdown its speaker ended with, what the room
//! asked and answered, and a cue file timed against the recording.

use std::io::{Cursor, Write};

use serde::Serialize;

use crate::images::Stored;
use crate::session::Cue;
use crate::wire::ScoreRow;

/// Everything `bundle` reads, copied out from under the registry lock.
///
/// Zipping an evening walks every deck and every picture. Doing that inside
/// `registry.with` held the one lock the whole instance shares for as long as
/// the zip took, so every other room waited on one host pressing save.
pub struct ExportView {
    pub opened: std::time::SystemTime,
    /// The host's own deck, whether it is on screen or parked behind a talk.
    pub host_markdown: String,
    pub staged: Option<u64>,
    /// What is live right now, which for a staged talk is the deck its speaker
    /// has been driving.
    pub live_markdown: String,
    pub talks: Vec<TalkView>,
    pub questions: Vec<QuestionView>,
    pub images: Vec<Stored>,
    pub board: Vec<ScoreRow>,
    pub timeline: Vec<Cue>,
    /// Every question that took votes, on the decks whose votes the room still
    /// holds: the host deck and the talk on stage.
    pub polls: Vec<PollView>,
    /// Whether `answers.csv` names who chose what. The host opts in.
    pub with_people: bool,
}

pub struct PollView {
    /// `None` for the host deck.
    pub talk: Option<u64>,
    pub talk_title: String,
    pub slide: usize,
    /// The slide's own text, tags stripped, so a row can be read without the
    /// deck beside it.
    pub prompt: String,
    pub options: Vec<String>,
    pub correct: Vec<usize>,
    pub counts: Vec<usize>,
    pub revealed: bool,
    /// Named voters and what they chose. Anonymous voters are counted above
    /// and not listed here.
    pub answers: Vec<(String, Vec<usize>)>,
}

pub struct TalkView {
    pub id: u64,
    pub title: String,
    pub by: String,
    pub markdown: String,
    pub dropped: bool,
}

pub struct QuestionView {
    pub text: String,
    pub votes: usize,
    pub answered: bool,
}

#[derive(Serialize)]
struct TalkRecord {
    id: Option<u64>,
    title: String,
    by: String,
    file: String,
    questions: Vec<QuestionRecord>,
}

#[derive(Serialize)]
struct QuestionRecord {
    text: String,
    votes: usize,
    answered: bool,
}

#[derive(Serialize)]
struct CueRecord {
    at: String,
    seconds: f64,
    talk: Option<u64>,
    title: String,
    slide: usize,
}

#[derive(Serialize)]
struct Record {
    opened: String,
    talks: Vec<TalkRecord>,
    board: Vec<crate::wire::ScoreRow>,
    timeline: Vec<CueRecord>,
}

/// One field of a csv row. Quoted whenever it has to be, and always for text
/// somebody typed, so a name holding a comma stays one field.
fn csv(field: &str) -> String {
    if field.is_empty() {
        return String::new();
    }
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// `votes.csv`: one row per option per question, with the count.
pub fn votes_csv(polls: &[PollView]) -> String {
    let mut out = String::from("talk,slide,prompt,option,text,votes,correct,revealed\n");
    for poll in polls {
        let talk = poll.talk.map_or("host".to_string(), |id| id.to_string());
        for (index, text) in poll.options.iter().enumerate() {
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                csv(&talk),
                poll.slide + 1,
                csv(&poll.prompt),
                index + 1,
                csv(text),
                poll.counts.get(index).copied().unwrap_or(0),
                if poll.correct.contains(&index) {
                    "yes"
                } else {
                    "no"
                },
                if poll.revealed { "yes" } else { "no" },
            ));
        }
    }
    out
}

/// `answers.csv`: one row per named voter per question. Options are numbered
/// as in `votes.csv`, several joined with `;`.
pub fn answers_csv(polls: &[PollView]) -> String {
    let mut out = String::from("talk,slide,prompt,name,chose,right\n");
    for poll in polls {
        let talk = poll.talk.map_or("host".to_string(), |id| id.to_string());
        let mut want = poll.correct.clone();
        want.sort_unstable();
        for (name, chosen) in &poll.answers {
            let mut picked = chosen.clone();
            picked.sort_unstable();
            let listed = picked
                .iter()
                .map(|o| (o + 1).to_string())
                .collect::<Vec<_>>()
                .join(";");
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv(&talk),
                poll.slide + 1,
                csv(&poll.prompt),
                csv(name),
                csv(&listed),
                if picked == want { "yes" } else { "no" },
            ));
        }
    }
    out
}

/// The text of a rendered slide, for a csv column: tags gone, whitespace
/// folded, the few entities the renderer writes put back.
pub fn plain(html: &str) -> String {
    const BLOCKS: [&str; 15] = [
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "div",
        "pre",
        "tr",
        "td",
        "th",
        "blockquote",
        "br",
    ];
    let mut out = String::new();
    let mut tag: Option<String> = None;
    for ch in html.chars() {
        match (&mut tag, ch) {
            (None, '<') => tag = Some(String::new()),
            (Some(name), '>') => {
                // A block boundary is a word boundary. An inline span is not,
                // or `kind: Service` would come apart around its colored words.
                let name = name.trim_start_matches('/');
                let name = name.split_whitespace().next().unwrap_or("");
                if BLOCKS.contains(&name) {
                    out.push(' ');
                }
                tag = None;
            }
            (Some(name), c) => name.push(c),
            (None, c) => out.push(c),
        }
    }
    let text = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    folded.chars().take(120).collect()
}

/// A file name from a title: lowercase, no spaces, nothing a filesystem or a
/// zip reader has to think about.
fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.extend(ch.to_lowercase());
        } else {
            gap = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    if out.is_empty() {
        "talk".to_string()
    } else {
        out
    }
}

fn stamp(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Enough to line a cue up with a recording, without pulling in a date
    // library for one field.
    format!("{secs}")
}

/// `HH:MM:SS.mmm` from the top of the evening, which is what a video editor
/// wants. Cues run to the next one, and the last runs a minute.
fn vtt_time(offset: f64) -> String {
    let ms = (offset.max(0.0) * 1000.0).round() as u64;
    let (h, m, s, milli) = (
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000,
    );
    format!("{h:02}:{m:02}:{s:02}.{milli:03}")
}

fn vtt(session: &ExportView) -> String {
    let start = session.opened;
    let mut out = String::from("WEBVTT\n\n");
    let cues: &[Cue] = &session.timeline;
    for (index, cue) in cues.iter().enumerate() {
        let from = cue
            .at
            .duration_since(start)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let to = cues
            .get(index + 1)
            .and_then(|next| next.at.duration_since(start).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(from + 60.0);
        out.push_str(&format!(
            "{}\n{} --> {}\n{} \u{2014} slide {}\n\n",
            index + 1,
            vtt_time(from),
            vtt_time(to),
            cue.title,
            cue.slide + 1
        ));
    }
    out
}

/// The whole evening as a zip. Stored rather than deflated: it is markdown and
/// a little json, and storing keeps the dependency free of a codec.
pub fn bundle(session: &ExportView) -> std::io::Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut talks = Vec::new();

    writer.start_file("00-host.md", options)?;
    writer.write_all(session.host_markdown.as_bytes())?;
    talks.push(TalkRecord {
        id: None,
        title: "Host".to_string(),
        by: String::new(),
        file: "00-host.md".to_string(),
        questions: Vec::new(),
    });

    // The running order as it stands. A talk the host took off was not part of
    // the evening, and numbering it would say it was.
    let running = session.talks.iter().filter(|t| !t.dropped);
    for (index, talk) in running.enumerate() {
        // A talk on stage is being driven live, so the live deck is the one the
        // speaker ended with.
        let markdown = if session.staged == Some(talk.id) {
            session.live_markdown.clone()
        } else {
            talk.markdown.clone()
        };
        let file = format!("{:02}-{}.md", index + 1, slug(&talk.title));
        writer.start_file(&file, options)?;
        writer.write_all(markdown.as_bytes())?;
        talks.push(TalkRecord {
            id: Some(talk.id),
            title: talk.title.clone(),
            by: talk.by.clone(),
            file,
            questions: Vec::new(),
        });
    }

    // Whatever is on the floor belongs to the talk that is up.
    if let Some(current) = talks
        .iter_mut()
        .find(|t| t.id == session.staged && session.staged.is_some())
    {
        current.questions = session
            .questions
            .iter()
            .map(|q| QuestionRecord {
                text: q.text.clone(),
                votes: q.votes,
                answered: q.answered,
            })
            .collect();
    }

    // The links in a deck point at a room that will not outlive the evening, so
    // the pictures travel with it. The file name carries the id the deck's own
    // url ends with.
    for held in &session.images {
        writer.start_file(format!("images/{}.{}", held.id, held.extension()), options)?;
        writer.write_all(&held.bytes)?;
    }

    let record = Record {
        opened: stamp(session.opened),
        talks,
        board: session.board.clone(),
        timeline: session
            .timeline
            .iter()
            .map(|cue| CueRecord {
                at: stamp(cue.at),
                seconds: cue
                    .at
                    .duration_since(session.opened)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0),
                talk: cue.talk,
                title: cue.title.clone(),
                slide: cue.slide,
            })
            .collect(),
    };
    writer.start_file("timeline.json", options)?;
    writer.write_all(serde_json::to_string_pretty(&record)?.as_bytes())?;

    writer.start_file("slides.vtt", options)?;
    writer.write_all(vtt(session).as_bytes())?;

    if !session.polls.is_empty() {
        writer.start_file("votes.csv", options)?;
        writer.write_all(votes_csv(&session.polls).as_bytes())?;
        if session.with_people {
            writer.start_file("answers.csv", options)?;
            writer.write_all(answers_csv(&session.polls).as_bytes())?;
        }
    }

    Ok(writer.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll() -> PollView {
        PollView {
            talk: None,
            talk_title: "Host".into(),
            slide: 1,
            prompt: "Year, \"roughly\"".into(),
            options: vec!["2012".into(), "20,15".into()],
            correct: vec![1],
            counts: vec![1, 2],
            revealed: true,
            answers: vec![("Ada, Countess".into(), vec![1]), ("Sam".into(), vec![0])],
        }
    }

    #[test]
    fn a_vote_row_per_option_with_text_quoted() {
        let csv = votes_csv(&[poll()]);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "talk,slide,prompt,option,text,votes,correct,revealed"
        );
        assert_eq!(
            lines.next().unwrap(),
            "\"host\",2,\"Year, \"\"roughly\"\"\",1,\"2012\",1,no,yes"
        );
        assert_eq!(
            lines.next().unwrap(),
            "\"host\",2,\"Year, \"\"roughly\"\"\",2,\"20,15\",2,yes,yes"
        );
    }

    #[test]
    fn an_answer_row_per_named_voter_says_whether_they_were_right() {
        let csv = answers_csv(&[poll()]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[1],
            "\"host\",2,\"Year, \"\"roughly\"\"\",\"Ada, Countess\",\"2\",yes"
        );
        assert_eq!(
            lines[2],
            "\"host\",2,\"Year, \"\"roughly\"\"\",\"Sam\",\"1\",no"
        );
    }

    #[test]
    fn a_slide_reads_as_its_text() {
        assert_eq!(
            plain("<h1>Year <em>Rust</em> 1.0 &amp; more</h1>\n<p>shipped?</p>"),
            "Year Rust 1.0 & more shipped?"
        );
        assert_eq!(
            plain("<pre><code><span class=\"a\">kind</span><span>: Service</span></code></pre>"),
            "kind: Service"
        );
    }
}
