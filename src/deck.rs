use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slide {
    pub html: String,
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
    /// How many presses this slide takes before the next one. Zero for a slide
    /// that arrives whole, which is every slide that holds no `*` list.
    #[serde(default)]
    pub steps: usize,
}

/// A slide carrying a task list becomes a question. `- [x]` marks an answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Question {
    pub options: Vec<String>,
    /// Whether more than one answer is right. Unlike `correct` this reaches the
    /// room, because a voter has to know they may pick several. It says how many
    /// without saying which.
    pub multi: bool,
    /// Indices of the right answers. Redacted for everyone but the presenter
    /// until the presenter reveals them.
    pub correct: Vec<usize>,
}

/// Slides split on a `---` line, speaker notes split from the body by `???`.
pub fn parse(markdown: &str) -> Vec<Slide> {
    let slides: Vec<Slide> = split_slides(markdown)
        .iter()
        .map(|raw| {
            let (body, notes) = split_notes(raw);
            let (prompt, question) = split_question(&body);
            let (html, steps) = stage_items(&render(&prompt), &fragments(&prompt));
            Slide {
                html,
                notes: notes.trim().to_string(),
                question,
                steps,
            }
        })
        .filter(|s| !(s.html.trim().is_empty() && s.notes.is_empty() && s.question.is_none()))
        .collect();

    if slides.is_empty() {
        return vec![Slide {
            html: String::new(),
            notes: String::new(),
            question: None,
            steps: 0,
        }];
    }
    slides
}

/// Tracks fenced code blocks.
///
/// A deck about software shows code, and code contains the very characters
/// this format uses: YAML separates documents with `---`, and `???` turns up in
/// regexes and stubs. Inside a fence they are content, not markup.
#[derive(Default)]
struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    /// Feeds one line and returns whether that line sits inside a fence, the
    /// opening and closing lines included.
    fn consume(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        // Four spaces already means an indented code block, not a fence.
        if indent >= 4 {
            return self.open.is_some();
        }
        let Some(marker) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else {
            return self.open.is_some();
        };
        let run = trimmed.chars().take_while(|c| *c == marker).count();
        if run < 3 {
            return self.open.is_some();
        }

        match self.open {
            None => {
                self.open = Some((marker, run));
                true
            }
            // A closing fence matches the opener and carries nothing else.
            Some((open_marker, open_run))
                if open_marker == marker && run >= open_run && trimmed[run..].trim().is_empty() =>
            {
                self.open = None;
                true
            }
            Some(_) => true,
        }
    }
}

/// A separator is a `---` line at the start or after a blank line, which keeps
/// a setext `Heading\n---` from splitting the slide in two.
fn split_slides(markdown: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut fence = Fence::default();

    for (i, line) in lines.iter().enumerate() {
        let fenced = fence.consume(line);
        let separator = line.trim_end() == "---" || line.trim_end() == "----";
        let standalone = i == 0 || lines[i - 1].trim().is_empty();
        if separator && standalone && !fenced {
            out.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
    }
    out.push(current.join("\n"));
    out
}

/// Pulls `- [ ]` and `- [x]` lines out of the body so the view can draw them as
/// buttons instead of a list, and so the answer can be held back.
///
/// Fenced lines are prompt, like everywhere else in this format. A slide that
/// shows what a question looks like is documentation, not a question.
fn split_question(body: &str) -> (String, Option<Question>) {
    let mut prompt = Vec::new();
    let mut options = Vec::new();
    let mut correct = Vec::new();
    let mut fence = Fence::default();

    for line in body.lines() {
        if fence.consume(line) {
            prompt.push(line);
            continue;
        }
        match task_item(line) {
            Some((checked, text)) => {
                if checked {
                    correct.push(options.len());
                }
                options.push(text);
            }
            None => prompt.push(line),
        }
    }

    if options.len() < 2 {
        return (body.to_string(), None);
    }
    let multi = correct.len() > 1;
    (
        prompt.join("\n"),
        Some(Question {
            options,
            multi,
            correct,
        }),
    )
}

fn task_item(line: &str) -> Option<(bool, String)> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let rest = rest.trim_start();
    let (marker, text) = if let Some(t) = rest.strip_prefix("[ ]") {
        (false, t)
    } else {
        let t = rest
            .strip_prefix("[x]")
            .or_else(|| rest.strip_prefix("[X]"))?;
        (true, t)
    };
    Some((marker, text.trim().to_string()))
}

fn split_notes(raw: &str) -> (String, String) {
    let mut fence = Fence::default();
    let lines: Vec<&str> = raw.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        if fence.consume(line) {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with("???") {
            continue;
        }
        let body = lines[..index].join("\n");
        let mut note = trimmed.trim_start_matches('?').trim_start().to_string();
        let rest = lines[index + 1..].join("\n");
        if !rest.trim().is_empty() {
            if !note.is_empty() {
                note.push('\n');
            }
            note.push_str(&rest);
        }
        return (body, note);
    }

    (raw.to_string(), String::new())
}

/// Which list items in this body come in one at a time, in the order the
/// renderer will emit them.
///
/// Marp's rule, because a deck written for Marp should behave the same here:
/// `*` and `1)` fragment, `-` and `1.` do not. One entry per list item line,
/// so the marker a line was written with is what decides.
fn fragments(body: &str) -> Vec<bool> {
    let mut out = Vec::new();
    let mut fence = Fence::default();
    for line in body.lines() {
        if fence.consume(line) {
            continue;
        }
        if let Some(marker) = list_marker(line) {
            out.push(marker == '*' || marker == ')');
        }
    }
    out
}

/// The character a list item was written with, or `)` for an ordered item that
/// used a bracket. `None` for a line that starts no item.
fn list_marker(line: &str) -> Option<char> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if matches!(first, '-' | '*' | '+') {
        // A marker needs a space after it, or `*emphasis*` opens a list.
        return chars
            .next()
            .filter(|c| *c == ' ' || *c == '\t')
            .map(|_| first);
    }
    if !first.is_ascii_digit() {
        return None;
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    let rest = &trimmed[digits..];
    let delimiter = rest.chars().next().filter(|c| *c == '.' || *c == ')')?;
    rest[1..]
        .chars()
        .next()
        .filter(|c| *c == ' ' || *c == '\t')
        .map(|_| delimiter)
}

/// Numbers the items that come in one at a time, and says how many there are.
///
/// The renderer emits one `<li>` per item in source order, so the nth opening
/// tag is the nth item the scan saw. Nothing else in the html can be a literal
/// `<li>`: a deck's own markup is escaped to text before it gets here.
fn stage_items(html: &str, fragments: &[bool]) -> (String, usize) {
    if !fragments.iter().any(|f| *f) {
        return (html.to_string(), 0);
    }
    let mut out = String::with_capacity(html.len() + fragments.len() * 24);
    let mut rest = html;
    let mut item = 0;
    let mut step = 0;

    while let Some(at) = rest.find("<li>") {
        out.push_str(&rest[..at]);
        if fragments.get(item).copied().unwrap_or(false) {
            step += 1;
            out.push_str(&format!("<li class=\"step\" data-step=\"{step}\">"));
        } else {
            out.push_str("<li>");
        }
        item += 1;
        rest = &rest[at + 4..];
    }
    out.push_str(rest);
    (out, step)
}

fn render(body: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let events = Parser::new_ext(body, options).map(|event| match event {
        // Anyone with the link can write a deck, so raw HTML is shown as text.
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: if is_safe_url(&dest_url) {
                dest_url
            } else {
                CowStr::Borrowed("")
            },
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: if is_safe_url(&dest_url) {
                dest_url
            } else {
                CowStr::Borrowed("")
            },
            title,
            id,
        }),
        other => other,
    });

    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

/// Blocks `javascript:` and `data:` hrefs, which would otherwise run when a
/// viewer taps a link in someone else's deck.
fn is_safe_url(url: &str) -> bool {
    let trimmed: String = url
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    let lowered = trimmed.to_ascii_lowercase();
    !(lowered.starts_with("javascript:")
        || lowered.starts_with("data:")
        || lowered.starts_with("vbscript:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_list_comes_in_one_item_at_a_time() {
        let slides = parse("# Why\n\n* First\n* Second\n* Third");
        assert_eq!(slides[0].steps, 3);
        assert!(
            slides[0]
                .html
                .contains(r#"<li class="step" data-step="1">"#)
        );
        assert!(
            slides[0]
                .html
                .contains(r#"<li class="step" data-step="3">"#)
        );
    }

    #[test]
    fn a_dash_list_arrives_whole() {
        let slides = parse("# Why\n\n- First\n- Second");
        assert_eq!(slides[0].steps, 0);
        assert!(!slides[0].html.contains("data-step"));
        assert!(slides[0].html.contains("<li>First</li>"));
    }

    /// Marp's rule for ordered lists: a bracket stages, a full stop does not.
    #[test]
    fn an_ordered_list_stages_on_a_bracket_and_not_on_a_stop() {
        assert_eq!(parse("1) One\n2) Two").pop().unwrap().steps, 2);
        assert_eq!(parse("1. One\n2. Two").pop().unwrap().steps, 0);
    }

    #[test]
    fn a_slide_mixing_both_stages_only_the_stars() {
        let slides = parse("- Always\n\n* Then this\n* Then that");
        assert_eq!(slides[0].steps, 2);
        assert!(slides[0].html.contains("<li>Always</li>"));
    }

    #[test]
    fn a_list_inside_a_fence_stages_nothing() {
        let slides = parse("# Code\n\n```md\n* one\n* two\n```");
        assert_eq!(slides[0].steps, 0, "a list in a fence is an example");
        assert!(!slides[0].html.contains("data-step"));
    }

    #[test]
    fn emphasis_is_not_a_list() {
        let slides = parse("*just emphasis* on its own line");
        assert_eq!(slides[0].steps, 0);
        assert!(slides[0].html.contains("<em>"));
    }

    #[test]
    fn a_nested_star_list_keeps_counting() {
        let slides = parse("* One\n  * Under one\n* Two");
        assert_eq!(slides[0].steps, 3);
        // Source order, so the nested item is the second step.
        let at = |n: u32| slides[0].html.find(&format!(r#"data-step="{n}""#)).unwrap();
        assert!(at(1) < at(2) && at(2) < at(3));
    }

    #[test]
    fn each_slide_counts_its_own_staging() {
        let slides = parse("* One\n* Two\n\n---\n\n# Plain\n\n---\n\n* Only one");
        assert_eq!(slides[0].steps, 2);
        assert_eq!(slides[1].steps, 0);
        assert_eq!(slides[2].steps, 1);
    }

    /// A question is drawn from its own list of options, not from the body, so
    /// the marker that makes a quiz cannot also stage it.
    #[test]
    fn a_question_is_not_staged_by_its_markers() {
        let slides = parse("# Pick\n\n* [ ] One\n* [x] Two");
        assert_eq!(slides[0].steps, 0);
        assert!(slides[0].question.is_some());
    }

    #[test]
    fn splits_on_a_separator_line() {
        let slides = parse("# One\n\n---\n\n# Two");
        assert_eq!(slides.len(), 2);
        assert!(slides[0].html.contains("One"));
        assert!(slides[1].html.contains("Two"));
    }

    #[test]
    fn keeps_a_setext_heading_whole() {
        let slides = parse("Heading\n---\n\nbody text");
        assert_eq!(slides.len(), 1);
        assert!(slides[0].html.contains("<h2>"));
    }

    #[test]
    fn pulls_speaker_notes_off_the_slide() {
        let slides = parse("# Title\n\n???\nremember the punchline");
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].notes, "remember the punchline");
        assert!(!slides[0].html.contains("punchline"));
    }

    #[test]
    fn a_task_list_inside_a_fence_stays_code() {
        let deck = "# How to ask\n\n```markdown\n- [x] right\n- [ ] wrong\n```";
        let slides = parse(deck);
        assert_eq!(slides.len(), 1);
        assert!(slides[0].question.is_none());
        assert!(slides[0].html.contains("[x] right"));
    }

    #[test]
    fn a_fenced_example_does_not_disarm_a_real_question_below_it() {
        let deck =
            "# Both\n\n```markdown\n- [x] example\n- [ ] example\n```\n\n- [ ] no\n- [x] yes";
        let slides = parse(deck);
        let question = slides[0].question.as_ref().expect("a question");
        assert_eq!(question.options, vec!["no", "yes"]);
        assert_eq!(question.correct, vec![1]);
    }

    #[test]
    fn an_empty_deck_still_has_one_slide() {
        assert_eq!(parse("").len(), 1);
    }

    #[test]
    fn raw_html_is_shown_not_executed() {
        let slides = parse("<script>alert(1)</script>");
        assert!(!slides[0].html.contains("<script>"));
        assert!(slides[0].html.contains("&lt;script&gt;"));
    }

    #[test]
    fn inline_html_is_shown_not_executed() {
        let slides = parse("hello <img src=x onerror=alert(1)> there");
        assert!(!slides[0].html.contains("<img"));
        assert!(slides[0].html.contains("&lt;img"));
    }

    #[test]
    fn javascript_hrefs_are_stripped() {
        let slides = parse("[tap me](javascript:alert(1))");
        assert!(!slides[0].html.to_ascii_lowercase().contains("javascript:"));
    }

    #[test]
    fn obfuscated_javascript_hrefs_are_stripped() {
        let slides = parse("[tap me](  JaVa\tScRiPt:alert(1))");
        assert!(!slides[0].html.to_ascii_lowercase().contains("javascript:"));
    }

    #[test]
    fn a_picture_somewhere_else_is_drawn() {
        let slides = parse("![A crab](https://example.com/ferris.png)");
        let html = &slides[0].html;
        assert!(
            html.contains("src=\"https://example.com/ferris.png\""),
            "{html}"
        );
        assert!(html.contains("alt=\"A crab\""), "{html}");
    }

    #[test]
    fn a_picture_source_is_stripped_the_same_way_a_link_is() {
        for deck in [
            "![x](javascript:alert(1))",
            "![x](data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=)",
            "![x](vbscript:msgbox)",
        ] {
            let html = parse(deck).pop().unwrap().html;
            assert!(html.contains("<img"), "not drawn as a picture: {html}");
            assert!(html.contains("src=\"\""), "a source survived: {html}");
        }
    }

    /// A source the format will not parse never becomes a picture at all, so
    /// what reaches the room is the text somebody typed.
    #[test]
    fn a_source_that_is_not_a_url_is_not_a_picture() {
        let html = parse("![x](  JaVa\tScRiPt:alert(1))").pop().unwrap().html;
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.to_ascii_lowercase().contains("src="), "{html}");
    }

    #[test]
    fn ordinary_links_survive() {
        let slides = parse("[docs](https://example.com/x)");
        assert!(slides[0].html.contains("https://example.com/x"));
    }

    #[test]
    fn a_task_list_becomes_a_question() {
        let slides = parse("# Year Rust 1.0 shipped?\n\n- [ ] 2012\n- [x] 2015\n- [ ] 2018");
        let question = slides[0].question.as_ref().expect("expected a question");
        assert_eq!(question.options, vec!["2012", "2015", "2018"]);
        assert_eq!(question.correct, vec![1]);
    }

    #[test]
    fn the_options_leave_the_rendered_prompt() {
        let slides = parse("# Pick one\n\n- [ ] a\n- [x] b");
        assert!(slides[0].html.contains("Pick one"));
        assert!(!slides[0].html.contains("[x]"));
    }

    #[test]
    fn several_right_answers_are_allowed() {
        let slides = parse("q\n\n- [x] a\n- [x] b\n- [ ] c");
        let question = slides[0].question.as_ref().unwrap();
        assert_eq!(question.correct, vec![0, 1]);
    }

    #[test]
    fn an_ordinary_bullet_list_is_not_a_question() {
        let slides = parse("# Points\n\n- one\n- two");
        assert!(slides[0].question.is_none());
        assert!(slides[0].html.contains("<ul>"));
    }

    #[test]
    fn a_single_option_is_not_a_question() {
        let slides = parse("# Nearly\n\n- [x] only one");
        assert!(slides[0].question.is_none());
    }

    #[test]
    fn a_yaml_separator_inside_a_fence_is_not_a_slide_break() {
        let deck =
            "# Config\n\n```yaml\napiVersion: v1\n\n---\n\nkind: Service\n```\n\nstill one slide";
        let slides = parse(deck);
        assert_eq!(slides.len(), 1, "a fenced --- split the deck");
        assert!(slides[0].html.contains("kind: Service"));
        assert!(slides[0].html.contains("still one slide"));
    }

    #[test]
    fn a_separator_after_a_fence_still_splits() {
        let deck = "# One\n\n```\ncode\n```\n\n---\n\n# Two";
        assert_eq!(parse(deck).len(), 2, "a real separator stopped working");
    }

    #[test]
    fn question_marks_inside_a_fence_are_not_speaker_notes() {
        let deck = "# Regex\n\n```python\npattern = r\"a???b\"\n???\nmore code\n```";
        let slides = parse(deck);
        assert_eq!(
            slides[0].notes, "",
            "code became speaker notes: {:?}",
            slides[0].notes
        );
        assert!(slides[0].html.contains("more code"));
    }

    #[test]
    fn notes_after_a_fence_still_work() {
        let deck = "# Talk\n\n```\ncode\n```\n\n???\nremember this";
        let slides = parse(deck);
        assert_eq!(slides[0].notes, "remember this");
        assert!(slides[0].html.contains("code"));
    }

    #[test]
    fn a_tilde_fence_counts_too() {
        let deck = "# T\n\n~~~\n\n---\n\n~~~\n\ntail";
        assert_eq!(
            parse(deck).len(),
            1,
            "a tilde fence did not protect the separator"
        );
    }

    #[test]
    fn an_unclosed_fence_swallows_the_rest_rather_than_splitting_it() {
        let deck = "# Oops\n\n```\nnever closed\n\n---\n\n# Two";
        assert_eq!(parse(deck).len(), 1);
    }
}
