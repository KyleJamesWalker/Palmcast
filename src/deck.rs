use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slide {
    pub html: String,
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
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
            Slide {
                html: render(&prompt),
                notes: notes.trim().to_string(),
                question,
            }
        })
        .filter(|s| !(s.html.trim().is_empty() && s.notes.is_empty() && s.question.is_none()))
        .collect();

    if slides.is_empty() {
        return vec![Slide {
            html: String::new(),
            notes: String::new(),
            question: None,
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
