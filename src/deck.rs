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
    /// How the deck leaves this slide for the next one, when the author asked
    /// for anything. The boundary belongs to the slide above it, so stepping
    /// back over the same boundary runs the same animation reversed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
}

/// A transition, as the deck named it. Whether the name is installed is the
/// view's business: it holds the stylesheets and this does not.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transition {
    pub name: String,
    /// Milliseconds. `None` leaves it to the transition's own stylesheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
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
    let mut slides: Vec<Slide> = Vec::new();
    // A plain `transition` keeps applying until another one replaces it, so it
    // outlives the slide that named it. An `_transition` never does.
    let mut carried: Option<Transition> = None;

    for raw in split_slides(markdown) {
        let (raw, asked) = split_directives(&raw);
        if let Some(named) = asked.transition {
            carried = Some(named);
        }
        let transition = asked.spot.or_else(|| carried.clone());

        let (body, notes) = split_notes(&raw);
        let (prompt, question) = split_question(&body);
        let (html, steps) = stage_items(&render(&prompt), &fragments(&prompt));
        let notes = notes.trim().to_string();
        // A slide holding nothing but a directive is not a slide, but it has
        // already had its say: `carried` is set above this line, not below it.
        if html.trim().is_empty() && notes.is_empty() && question.is_none() {
            continue;
        }
        slides.push(Slide {
            html,
            notes,
            question,
            steps,
            transition,
        });
    }

    if slides.is_empty() {
        return vec![Slide {
            html: String::new(),
            notes: String::new(),
            question: None,
            steps: 0,
            transition: None,
        }];
    }
    slides
}

/// The theme the deck asked for, if it asked for one and the name is a name.
///
/// Deck wide wherever it is written, and the last one wins, because a theme
/// that changed halfway through would repaint the room mid-talk.
pub fn theme_of(markdown: &str) -> Option<String> {
    let mut found = None;
    let mut fence = Fence::default();
    for line in markdown.lines() {
        if fence.consume(line) {
            continue;
        }
        if let Some(("theme", value)) = directive(line)
            && let Some(name) = style_name(value)
        {
            found = Some(name);
        }
    }
    found
}

#[derive(Default)]
struct Directives {
    transition: Option<Transition>,
    spot: Option<Transition>,
}

/// Lifts the directive lines out of a slide and reads them.
///
/// They are stripped whether or not the value was usable. A deck that misspells
/// a theme has made a mistake worth ignoring, not one worth printing across the
/// slide in front of the room.
fn split_directives(raw: &str) -> (String, Directives) {
    let mut body = Vec::new();
    let mut asked = Directives::default();
    let mut fence = Fence::default();

    for line in raw.lines() {
        if fence.consume(line) {
            body.push(line);
            continue;
        }
        match directive(line) {
            Some(("transition", value)) => asked.transition = transition(value),
            Some(("_transition", value)) => asked.spot = transition(value),
            // Read deck wide by `theme_of`, and dropped here so it never draws.
            Some(("theme", _)) => {}
            _ => body.push(line),
        }
    }
    (body.join("\n"), asked)
}

/// A `<!-- name: value -->` line on its own. Any other comment is left alone,
/// which means it renders as the text a deck's raw html already renders as.
fn directive(line: &str) -> Option<(&str, &str)> {
    let inner = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?;
    let (name, value) = inner.split_once(':')?;
    Some((name.trim(), value.trim()))
}

/// `name` or `name <duration>`. A second word that is not a duration voids the
/// whole thing rather than being ignored, so a typo is silent rather than half
/// obeyed.
fn transition(value: &str) -> Option<Transition> {
    let mut words = value.split_whitespace();
    let name = style_name(words.next()?)?;
    let duration = match words.next() {
        Some(raw) => Some(duration_ms(raw)?),
        None => None,
    };
    words
        .next()
        .is_none()
        .then_some(Transition { name, duration })
}

/// A name reaches a url and a class attribute, so the alphabet is narrow on
/// purpose: anything outside it is refused here rather than escaped later.
pub(crate) fn style_name(value: &str) -> Option<String> {
    let name = value.trim();
    let shaped = !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    shaped.then(|| name.to_string())
}

fn duration_ms(value: &str) -> Option<u32> {
    if let Some(count) = value.strip_suffix("ms") {
        return count.parse().ok();
    }
    let seconds: f64 = value.strip_suffix('s')?.parse().ok()?;
    (seconds.is_finite() && (0.0..=60.0).contains(&seconds))
        .then(|| (seconds * 1000.0).round() as u32)
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

    #[test]
    fn a_transition_directive_names_the_slide_it_sits_on() {
        let slides = parse("<!-- transition: fade -->\n# Why Rust");
        assert_eq!(
            slides[0].transition.as_ref().map(|t| t.name.as_str()),
            Some("fade")
        );
    }

    #[test]
    fn a_directive_line_is_not_drawn_on_the_slide() {
        let slides = parse("<!-- transition: fade -->\n# Why Rust");
        assert!(!slides[0].html.contains("transition"), "{}", slides[0].html);
        assert!(slides[0].html.contains("Why Rust"));
    }

    #[test]
    fn a_transition_carries_on_to_the_slides_after_it() {
        let slides = parse("<!-- transition: cover -->\n# One\n\n---\n\n# Two");
        assert_eq!(
            slides[1].transition.as_ref().map(|t| t.name.as_str()),
            Some("cover")
        );
    }

    #[test]
    fn an_underscored_transition_applies_to_its_own_slide_only() {
        let deck = "<!-- transition: cover -->\n# One\n\n---\n\n<!-- _transition: none -->\n# Two\n\n---\n\n# Three";
        let slides = parse(deck);
        assert_eq!(
            slides[1].transition.as_ref().map(|t| t.name.as_str()),
            Some("none")
        );
        assert_eq!(
            slides[2].transition.as_ref().map(|t| t.name.as_str()),
            Some("cover")
        );
    }

    #[test]
    fn a_duration_rides_with_the_name() {
        let slides = parse(
            "<!-- transition: fade 1s -->\n# One\n\n---\n\n<!-- transition: fade 250ms -->\n# Two",
        );
        assert_eq!(slides[0].transition.as_ref().unwrap().duration, Some(1000));
        assert_eq!(slides[1].transition.as_ref().unwrap().duration, Some(250));
    }

    #[test]
    fn a_theme_is_read_wherever_it_is_written() {
        assert_eq!(
            theme_of("# One\n\n---\n\n<!-- theme: paper -->\n# Two").as_deref(),
            Some("paper")
        );
    }

    #[test]
    fn the_last_theme_wins() {
        assert_eq!(
            theme_of("<!-- theme: neon -->\n# One\n\n---\n\n<!-- theme: paper -->\n# Two")
                .as_deref(),
            Some("paper")
        );
    }

    #[test]
    fn a_name_that_could_be_a_path_or_markup_is_refused() {
        for bad in ["../secret", "a/b", "fade\"", "<script>", "UPPER", "a b"] {
            let deck = format!("<!-- transition: {bad} -->\n# One");
            assert_eq!(parse(&deck)[0].transition, None, "accepted {bad}");
            let deck = format!("<!-- theme: {bad} -->\n# One");
            assert_eq!(theme_of(&deck), None, "accepted {bad}");
        }
    }

    #[test]
    fn a_directive_inside_a_fence_is_code() {
        let deck = "# Docs\n\n```html\n<!-- transition: fade -->\n```";
        let slides = parse(deck);
        assert_eq!(slides[0].transition, None);
        assert!(
            slides[0].html.contains("transition"),
            "the example was eaten"
        );
    }

    #[test]
    fn a_deck_with_no_directives_says_so() {
        let slides = parse("# Plain");
        assert_eq!(slides[0].transition, None);
        assert_eq!(theme_of("# Plain"), None);
    }

    /// The sample deck shows the directives off in a bullet, so a line that
    /// merely mentions one has to stay a line that mentions one.
    #[test]
    fn a_directive_quoted_in_a_bullet_is_text() {
        let slides = parse("# Rules\n\n- `<!-- theme: neon -->` paints it");
        assert_eq!(slides[0].transition, None);
        assert_eq!(theme_of("- `<!-- theme: neon -->` paints it"), None);
        assert!(
            slides[0].html.contains("theme: neon"),
            "the example was eaten: {}",
            slides[0].html
        );
    }

    #[test]
    fn a_directive_needs_the_line_to_itself() {
        let slides = parse("Text before <!-- transition: fade --> and after");
        assert_eq!(slides[0].transition, None);
    }
}
