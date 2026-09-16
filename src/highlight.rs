//! Colors a fenced code block. Class names only: the theme decides what each
//! one looks like, and the text is escaped on the way through.

use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// A block past either of these renders plain. Highlighting is regex work on
/// text a stranger wrote, and a deck is slides, not a source tree.
const MAX_BYTES: usize = 16 * 1024;
const MAX_LINES: usize = 400;

const STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The block as spans, or `None` when the language names no grammar or the
/// block is too big to be worth it. Text is escaped either way by the caller
/// that falls back, so `None` is never a leak.
pub fn highlight(lang: &str, code: &str) -> Option<String> {
    let token = lang.split_whitespace().next()?.trim();
    if token.is_empty() || code.len() > MAX_BYTES || code.lines().count() > MAX_LINES {
        return None;
    }
    let set = syntaxes();
    let syntax = set
        .find_syntax_by_token(token)
        .or_else(|| set.find_syntax_by_extension(token))?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, set, STYLE);
    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_language_comes_back_as_classed_spans() {
        let out = highlight("rust", "fn main() {}\n").unwrap();
        assert!(out.contains("hl-storage"), "{out}");
        assert!(out.contains("hl-entity"), "{out}");
        assert!(out.contains(">main<"));
    }

    #[test]
    fn markup_inside_code_is_text_and_not_markup() {
        let out = highlight("html", "<script>alert(1)</script>\n").unwrap();
        assert!(!out.contains("<script"), "{out}");
        assert!(out.contains("&lt;"), "{out}");
    }

    #[test]
    fn an_unknown_language_is_left_to_the_plain_path() {
        assert!(highlight("nosuchlang", "x = 1\n").is_none());
        assert!(highlight("", "x = 1\n").is_none());
    }

    #[test]
    fn a_block_too_big_to_be_a_slide_is_left_plain() {
        let big = "x\n".repeat(MAX_LINES + 1);
        assert!(highlight("rust", &big).is_none());
        let wide = format!("{}\n", "x".repeat(MAX_BYTES + 1));
        assert!(highlight("rust", &wide).is_none());
    }

    #[test]
    fn a_language_by_extension_or_name_both_answer() {
        assert!(highlight("py", "def f(): pass\n").is_some());
        assert!(highlight("python", "def f(): pass\n").is_some());
    }
}
