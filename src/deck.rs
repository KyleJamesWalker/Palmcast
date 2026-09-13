use pulldown_cmark::{Options, Parser, html};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slide {
    pub html: String,
    pub notes: String,
}

/// Slides split on a `---` line, speaker notes split from the body by `???`.
pub fn parse(markdown: &str) -> Vec<Slide> {
    let slides: Vec<Slide> = split_slides(markdown)
        .iter()
        .map(|raw| {
            let (body, notes) = split_notes(raw);
            Slide {
                html: render(body),
                notes: notes.trim().to_string(),
            }
        })
        .filter(|s| !(s.html.trim().is_empty() && s.notes.is_empty()))
        .collect();

    if slides.is_empty() {
        return vec![Slide {
            html: String::new(),
            notes: String::new(),
        }];
    }
    slides
}

/// A separator is a `---` line at the start or after a blank line, which keeps
/// a setext `Heading\n---` from splitting the slide in two.
fn split_slides(markdown: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let fence = line.trim_end() == "---" || line.trim_end() == "----";
        let standalone = i == 0 || lines[i - 1].trim().is_empty();
        if fence && standalone {
            out.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
    }
    out.push(current.join("\n"));
    out
}

fn split_notes(raw: &str) -> (&str, &str) {
    match raw.find("\n???") {
        Some(i) => {
            let rest = &raw[i + 4..];
            (&raw[..i], rest)
        }
        None if raw.trim_start().starts_with("???") => {
            let i = raw.find("???").unwrap();
            (&raw[..i], &raw[i + 3..])
        }
        None => (raw, ""),
    }
}

fn render(body: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(body, options));
    out
}
