//! Reads a Marp or reveal.js deck and writes it in this application's format.

use serde::Serialize;

use crate::deck::{Fence, style_name};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Marp,
    Reveal,
    /// Front matter and nothing else that marks a tool. Read like Marp.
    Plain,
}

#[derive(Debug, Serialize)]
pub struct Converted {
    pub source: Source,
    pub markdown: String,
    /// One line per change or drop.
    pub changes: Vec<String>,
}

/// Which tool wrote a deck, from its front matter and its habits.
pub fn detect(markdown: &str) -> Option<Source> {
    let (front, body) = split_front_matter(markdown);
    if front.iter().any(|(k, v)| k == "marp" && v == "true") {
        return Some(Source::Marp);
    }
    let reveal_front = front.iter().any(|(k, _)| {
        matches!(
            k.as_str(),
            "separator" | "verticalSeparator" | "revealOptions"
        )
    });
    let reveal_body = body.lines().any(|line| {
        let t = line.trim();
        t == "--"
            || t.starts_with("Note:")
            || t.starts_with("Notes:")
            || t.contains("<!-- .slide:")
            || t.contains("<!-- .element:")
    });
    if reveal_front || reveal_body {
        return Some(Source::Reveal);
    }
    let marp_body = body.lines().any(|line| {
        let t = line.trim();
        t.starts_with("<!-- _class:")
            || t.starts_with("<!-- paginate")
            || t.starts_with("<!-- backgroundColor")
            || t.starts_with("<!-- _backgroundColor")
            || t.starts_with("![bg")
    });
    if marp_body {
        return Some(Source::Marp);
    }
    (!front.is_empty()).then_some(Source::Plain)
}

/// Rewrites a deck for this application. A deck no other tool wrote comes back
/// unchanged, as `Plain`, with no changes listed.
pub fn convert(markdown: &str) -> Converted {
    let Some(source) = detect(markdown) else {
        return Converted {
            source: Source::Plain,
            markdown: markdown.to_string(),
            changes: Vec::new(),
        };
    };
    let (front, body) = split_front_matter(markdown);
    let mut changes = Vec::new();
    let mut head = Vec::new();

    for (key, value) in &front {
        match key.as_str() {
            "theme" => match theme_for(source, value) {
                Some(theme) => {
                    if theme != *value {
                        changes.push(format!("theme `{value}` is `{theme}` here"));
                    }
                    head.push(format!("<!-- theme: {theme} -->"));
                }
                None => changes.push(format!("theme `{value}` has no match here, dropped")),
            },
            "transition" => match transition_line(value, false) {
                Some(line) => head.push(line),
                None => changes.push(format!("transition `{value}` is not a name, dropped")),
            },
            other => changes.push(format!(
                "front matter `{other}` has no meaning here, dropped"
            )),
        }
    }
    let heading_divider = front
        .iter()
        .find(|(k, _)| k == "headingDivider")
        .and_then(|(_, v)| {
            v.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .next_back()
        })
        .and_then(|v| v.trim().parse::<usize>().ok());

    let body = match source {
        Source::Reveal => reveal_body(&body, &mut changes),
        Source::Marp | Source::Plain => marp_body(&body, heading_divider, &mut changes),
    };

    let mut out = String::new();
    if !head.is_empty() {
        out.push_str(&head.join("\n"));
        out.push_str("\n\n");
    }
    out.push_str(body.trim_matches('\n'));
    out.push('\n');
    Converted {
        source,
        markdown: out,
        changes,
    }
}

/// `---` on the first line opens front matter and the next `---` closes it.
/// An opener with no closer is a deck that starts with a separator.
fn split_front_matter(markdown: &str) -> (Vec<(String, String)>, String) {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (Vec::new(), markdown.to_string());
    }
    let mut pairs = Vec::new();
    let mut rest = Vec::new();
    let mut closed = false;
    for line in lines {
        if !closed {
            if line.trim() == "---" {
                closed = true;
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                pairs.push((key.trim().to_string(), value.to_string()));
            }
            continue;
        }
        rest.push(line);
    }
    if !closed {
        return (Vec::new(), markdown.to_string());
    }
    (pairs, rest.join("\n"))
}

/// The theme here that stands in for one of theirs. Any other name is kept
/// and checked when the deck is parsed.
fn theme_for(source: Source, theirs: &str) -> Option<String> {
    let mapped = match (source, theirs) {
        (Source::Marp, "default") => "daylight",
        (Source::Marp, "gaia") => "paper",
        (Source::Marp, "uncover") => "bold",
        (Source::Reveal, "black" | "night" | "moon") => "ember",
        (Source::Reveal, "white" | "simple" | "sky") => "daylight",
        (Source::Reveal, "league" | "blood") => "bold",
        (Source::Reveal, "beige" | "serif" | "solarized") => "paper",
        (Source::Reveal, "dracula") => "neon",
        (_, other) => other,
    };
    style_name(mapped)
}

fn transition_line(value: &str, local: bool) -> Option<String> {
    let mut words = value.split_whitespace();
    let name = style_name(words.next()?)?;
    let duration = words.next().map(|d| format!(" {d}")).unwrap_or_default();
    let key = if local { "_transition" } else { "transition" };
    Some(format!("<!-- {key}: {name}{duration} -->"))
}

/// Marp's directive names. Any other comment in a Marp deck is a note.
const MARP_DIRECTIVES: [&str; 21] = [
    "theme",
    "style",
    "headingDivider",
    "size",
    "math",
    "paginate",
    "header",
    "footer",
    "class",
    "backgroundColor",
    "backgroundImage",
    "backgroundPosition",
    "backgroundRepeat",
    "backgroundSize",
    "color",
    "transition",
    "title",
    "author",
    "description",
    "keywords",
    "url",
];

/// Words Marp reads out of an image's alt text.
const MARP_IMAGE_WORDS: [&str; 12] = [
    "bg",
    "left",
    "right",
    "fit",
    "cover",
    "contain",
    "auto",
    "vertical",
    "blur",
    "brightness",
    "grayscale",
    "invert",
];

fn flush_notes(out: &mut Vec<String>, notes: &mut Vec<String>) {
    if notes.is_empty() {
        return;
    }
    out.push(String::new());
    out.push("???".to_string());
    out.append(notes);
}

/// A separator splits only after a blank line, and a note block flushed just
/// before it would otherwise sit flush against it.
fn separator(out: &mut Vec<String>) {
    if out.last().is_some_and(|last| !last.trim().is_empty()) {
        out.push(String::new());
    }
    out.push("---".to_string());
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn marp_body(body: &str, heading_divider: Option<usize>, changes: &mut Vec<String>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut fence = Fence::default();
    let mut in_style = false;
    let mut in_comment: Option<Vec<String>> = None;
    let mut dropped_style = false;
    let mut dropped_directives = 0;
    let mut backgrounds = 0;
    let mut fits = 0;
    let mut seen_content = false;

    for raw in body.lines() {
        if fence.consume(raw) {
            out.push(raw.to_string());
            seen_content = true;
            continue;
        }
        let trimmed = raw.trim();

        if in_style {
            if trimmed.contains("</style>") {
                in_style = false;
            }
            continue;
        }
        if trimmed.starts_with("<style") {
            dropped_style = true;
            in_style = !trimmed.contains("</style>");
            continue;
        }

        if let Some(lines) = in_comment.as_mut() {
            match trimmed.find("-->") {
                Some(end) => {
                    let last = trimmed[..end].trim();
                    if !last.is_empty() {
                        lines.push(last.to_string());
                    }
                    notes.extend(in_comment.take().unwrap_or_default());
                }
                None => lines.push(trimmed.to_string()),
            }
            continue;
        }

        if trimmed == "---" {
            flush_notes(&mut out, &mut notes);
            separator(&mut out);
            continue;
        }

        if let Some(inner) = trimmed.strip_prefix("<!--") {
            let Some(end) = inner.find("-->") else {
                let first = inner.trim();
                in_comment = Some(if first.is_empty() {
                    Vec::new()
                } else {
                    vec![first.to_string()]
                });
                continue;
            };
            let inner = inner[..end].trim();
            if inner.is_empty() {
                continue;
            }
            match marp_directive(inner) {
                Some(("transition" | "_transition", value)) => {
                    let local = inner.starts_with('_');
                    match transition_line(value, local) {
                        Some(line) => out.push(line),
                        None => dropped_directives += 1,
                    }
                }
                Some(("theme", value)) => match theme_for(Source::Marp, value) {
                    Some(theme) => {
                        if theme != value {
                            changes.push(format!("theme `{value}` is `{theme}` here"));
                        }
                        out.push(format!("<!-- theme: {theme} -->"));
                    }
                    None => changes.push(format!("theme `{value}` has no match here, dropped")),
                },
                Some(_) => dropped_directives += 1,
                None => notes.push(inner.to_string()),
            }
            continue;
        }

        let mut line = raw.to_string();
        if line.contains("<!-- fit -->") {
            line = line.replace("<!-- fit -->", "").trim_end().to_string();
            fits += 1;
        }
        if let Some(rewritten) = marp_image(&line) {
            backgrounds += 1;
            line = rewritten;
        }

        if let Some(level) = heading_divider
            && seen_content
            && heading_level(&line).is_some_and(|h| h <= level)
            && out.last().is_some_and(|last| last.trim() != "---")
        {
            flush_notes(&mut out, &mut notes);
            out.push(String::new());
            out.push("---".to_string());
            out.push(String::new());
        }
        if !trimmed.is_empty() {
            seen_content = true;
        }
        out.push(line);
    }
    flush_notes(&mut out, &mut notes);

    if dropped_style {
        changes.push(
            "a `<style>` block was dropped: a deck names a look and never carries CSS".into(),
        );
    }
    if dropped_directives > 0 {
        changes.push(format!(
            "{dropped_directives} Marp directive line{} dropped (class, paginate, background and the like)",
            plural(dropped_directives)
        ));
    }
    if backgrounds > 0 {
        changes.push(format!(
            "{backgrounds} background or sized image{} drawn as ordinary pictures",
            plural(backgrounds)
        ));
    }
    if fits > 0 {
        changes.push(format!(
            "{fits} `<!-- fit -->` mark{} removed",
            plural(fits)
        ));
    }
    if heading_divider.is_some() {
        changes.push("headingDivider applied as `---` before each heading it named".into());
    }
    out.join("\n")
}

/// `name: value` inside a comment, when `name` is one of Marp's.
fn marp_directive(inner: &str) -> Option<(&str, &str)> {
    let (name, value) = inner.split_once(':')?;
    let name = name.trim();
    let bare = name.trim_start_matches('_');
    MARP_DIRECTIVES
        .contains(&bare)
        .then(|| (name, value.trim()))
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && hashes <= 6 && trimmed[hashes..].starts_with(' ')).then_some(hashes)
}

/// `![bg right:40% w:200](x.png)` becomes `![](x.png)`. Alt words that are
/// not Marp keywords stay.
fn marp_image(line: &str) -> Option<String> {
    let start = line.find("![")?;
    let close = line[start..].find("](")? + start;
    let alt = &line[start + 2..close];
    let words: Vec<&str> = alt.split_whitespace().collect();
    let keyword = |w: &str| {
        let head = w.split(':').next().unwrap_or(w);
        MARP_IMAGE_WORDS.contains(&head) || w.ends_with('%') || w.ends_with("px")
    };
    if !words.iter().any(|w| keyword(w)) {
        return None;
    }
    let kept: Vec<&str> = words.into_iter().filter(|w| !keyword(w)).collect();
    Some(format!(
        "{}![{}{}",
        &line[..start],
        kept.join(" "),
        &line[close..]
    ))
}

fn reveal_body(body: &str, changes: &mut Vec<String>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut in_notes = false;
    let mut fence = Fence::default();
    let mut verticals = 0;
    let mut attributes = 0;
    let mut fragments = 0;

    for raw in body.lines() {
        if fence.consume(raw) {
            out.push(raw.to_string());
            continue;
        }
        let trimmed = raw.trim();
        if trimmed == "---" || trimmed == "--" {
            flush_notes(&mut out, &mut notes);
            in_notes = false;
            if trimmed == "--" {
                verticals += 1;
            }
            separator(&mut out);
            continue;
        }
        if in_notes {
            notes.push(raw.to_string());
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("Notes:")
            .or_else(|| trimmed.strip_prefix("Note:"))
        {
            in_notes = true;
            let rest = rest.trim();
            if !rest.is_empty() {
                notes.push(rest.to_string());
            }
            continue;
        }
        let mut line = raw.to_string();
        if line.contains("<!-- .element:") || line.contains("<!-- .slide:") {
            let fragment = line.contains("fragment");
            line = strip_comments(&line);
            attributes += 1;
            if fragment && let Some(bulleted) = stage_bullet(&line) {
                line = bulleted;
                fragments += 1;
            }
            if line.trim().is_empty() {
                continue;
            }
        }
        out.push(line);
    }
    flush_notes(&mut out, &mut notes);

    if verticals > 0 {
        changes.push(format!(
            "{verticals} vertical slide{} laid out in the run of the deck",
            plural(verticals)
        ));
    }
    if attributes > 0 {
        changes.push(format!(
            "{attributes} `.slide` or `.element` attribute comment{} dropped",
            plural(attributes)
        ));
    }
    if fragments > 0 {
        changes.push(format!(
            "{fragments} fragment{} kept as `*` items, which arrive one press at a time",
            plural(fragments)
        ));
    }
    out.join("\n")
}

fn strip_comments(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out.trim_end().to_string(),
        }
    }
    out.push_str(rest);
    out.trim_end().to_string()
}

/// `- item` to `* item`, the marker that arrives on a press.
fn stage_bullet(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let rest = line[indent..].strip_prefix("- ")?;
    Some(format!("{}* {rest}", &line[..indent]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARP: &str = "---\nmarp: true\ntheme: gaia\npaginate: true\ntransition: fade\n---\n\n<!-- _class: lead -->\n# Title <!-- fit -->\n\n![bg right:40%](hero.png)\n\n<!-- Remember to breathe -->\n\n---\n\n## Two\n\n- a\n- b\n\n<style>\nsection { color: red }\n</style>\n";

    #[test]
    fn a_marp_deck_is_detected_and_its_front_matter_becomes_directives() {
        assert_eq!(detect(MARP), Some(Source::Marp));
        let out = convert(MARP);
        assert_eq!(out.source, Source::Marp);
        assert!(
            out.markdown
                .starts_with("<!-- theme: paper -->\n<!-- transition: fade -->\n\n"),
            "{}",
            out.markdown
        );
        assert!(
            out.changes.iter().any(|c| c.contains("gaia")),
            "{:?}",
            out.changes
        );
        assert!(
            out.changes.iter().any(|c| c.contains("paginate")),
            "{:?}",
            out.changes
        );
    }

    #[test]
    fn marp_notes_directives_backgrounds_and_style_are_carried_or_dropped() {
        let out = convert(MARP);
        let slides = crate::deck::parse(&out.markdown);
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].notes, "Remember to breathe", "{}", out.markdown);
        assert!(
            slides[0].html.contains("<h1>Title</h1>"),
            "{}",
            slides[0].html
        );
        assert!(
            slides[0].html.contains("src=\"hero.png\""),
            "{}",
            slides[0].html
        );
        assert!(!out.markdown.contains("_class"), "{}", out.markdown);
        assert!(!out.markdown.contains("<style"), "{}", out.markdown);
        assert!(
            out.changes.iter().any(|c| c.contains("<style>")),
            "{:?}",
            out.changes
        );
    }

    #[test]
    fn a_marp_heading_divider_splits_the_deck() {
        let deck = "---\nmarp: true\nheadingDivider: 2\n---\n\n# One\n\ntext\n\n## Two\n\nmore\n\n### Not a split\n";
        let out = convert(deck);
        assert_eq!(
            crate::deck::parse(&out.markdown).len(),
            2,
            "{}",
            out.markdown
        );
    }

    #[test]
    fn a_multiline_marp_comment_is_a_note() {
        let deck = "---\nmarp: true\n---\n\n# One\n\n<!--\nfirst line\nsecond line\n-->\n";
        let out = convert(deck);
        assert_eq!(
            crate::deck::parse(&out.markdown)[0].notes,
            "first line\nsecond line",
            "{}",
            out.markdown
        );
    }

    const REVEAL: &str = "---\ntheme: black\nseparator: ^\\n---\\n$\n---\n\n# Intro\n\nNote: say hello\nand smile\n\n---\n\n## Points\n\n- one <!-- .element: class=\"fragment\" -->\n- two\n\n--\n\n## Deeper <!-- .slide: data-background=\"#fff\" -->\n";

    #[test]
    fn a_reveal_deck_is_detected_and_notes_verticals_and_attributes_are_handled() {
        assert_eq!(detect(REVEAL), Some(Source::Reveal));
        let out = convert(REVEAL);
        assert_eq!(out.source, Source::Reveal);
        assert!(
            out.markdown.starts_with("<!-- theme: ember -->"),
            "{}",
            out.markdown
        );
        let slides = crate::deck::parse(&out.markdown);
        assert_eq!(slides.len(), 3, "{}", out.markdown);
        assert_eq!(slides[0].notes, "say hello\nand smile");
        assert_eq!(slides[1].steps, 1, "{}", out.markdown);
        assert!(
            slides[2].html.contains("<h2>Deeper</h2>"),
            "{}",
            slides[2].html
        );
        assert!(!out.markdown.contains(".slide:"));
    }

    #[test]
    fn a_plain_deck_is_left_alone() {
        let deck = "# Mine\n\n---\n\n# Yours\n";
        assert_eq!(detect(deck), None);
        let out = convert(deck);
        assert_eq!(out.markdown, deck);
        assert!(out.changes.is_empty());
    }

    #[test]
    fn a_deck_that_merely_opens_with_a_separator_has_no_front_matter() {
        assert_eq!(detect("---\n\n# One\n\n---\n\n# Two\n"), None);
    }

    #[test]
    fn marp_markers_inside_a_fence_are_code() {
        let deck =
            "---\nmarp: true\n---\n\n# Docs\n\n```md\n<!-- _class: lead -->\n![bg](x.png)\n```\n";
        let out = convert(deck);
        assert!(
            out.markdown.contains("<!-- _class: lead -->"),
            "{}",
            out.markdown
        );
        assert!(out.markdown.contains("![bg](x.png)"), "{}", out.markdown);
    }
}
