//! The evening, in a file you can keep.
//!
//! One zip: every talk as the markdown its speaker ended with, what the room
//! asked and answered, and a cue file timed against the recording.

use std::io::{Cursor, Write};

use serde::Serialize;

use crate::session::{Cue, Session};

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

fn vtt(session: &Session) -> String {
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
pub fn bundle(session: &Session) -> std::io::Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut talks = Vec::new();

    // The host deck, whether it is on screen or parked behind a talk.
    let host_markdown = match (&session.parked, session.staged) {
        (Some(parked), Some(_)) => parked.markdown.clone(),
        _ => session.markdown.clone(),
    };
    writer.start_file("00-host.md", options)?;
    writer.write_all(host_markdown.as_bytes())?;
    talks.push(TalkRecord {
        id: None,
        title: "Host".to_string(),
        by: String::new(),
        file: "00-host.md".to_string(),
        questions: Vec::new(),
    });

    for (index, talk) in session.lineup.iter().enumerate() {
        // A talk on stage is being driven live, so the live deck is the one the
        // speaker ended with.
        let markdown = if session.staged == Some(talk.id) {
            session.markdown.clone()
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
                votes: q.voters.len(),
                answered: q.answered,
            })
            .collect();
    }

    let record = Record {
        opened: stamp(session.opened),
        talks,
        board: session.board(),
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

    Ok(writer.finish()?.into_inner())
}
