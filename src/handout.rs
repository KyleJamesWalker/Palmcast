//! A deck as one file: every slide, the theme it named, and the pictures the
//! room held, with nothing left that points back at the room. Printing it is
//! the way to a PDF.

use base64::Engine as _;

use crate::assets::Web;
use crate::deck::{self, Poll, Slide};
use crate::images::Stored;
use crate::styles::Styles;

pub struct Handout<'a> {
    pub markdown: &'a str,
    pub styles: &'a Styles,
    /// The room's own pictures, inlined so the file outlives the room.
    pub session: Option<&'a str>,
    pub images: &'a [Stored],
    pub with_notes: bool,
}

const PAGE_CSS: &str = r#"
html, body { margin: 0; }
body.handout { background: var(--ground); color: var(--ink); }
.page {
  box-sizing: border-box;
  min-height: 100vh;
  padding: 6vh 8vw;
  display: flex;
  flex-direction: column;
  justify-content: center;
  break-after: page;
  page-break-after: always;
}
.page:last-child { break-after: auto; page-break-after: auto; }
.handout .slide { font-size: clamp(20px, 3.6vw, 40px); }
.handout-options { list-style: none; padding: 0; margin: 1em 0 0; font-size: .7em; }
.handout-options li { padding: .4em .8em; border: 1px solid var(--edge); border-radius: 10px; margin: .3em 0; }
.handout-options li.right { border-color: var(--accent); }
.handout-poll { margin: 1em 0 0; font-size: .6em; color: var(--ink-dim); }
.handout-notes {
  margin-top: 1.2em;
  padding-top: .6em;
  border-top: 1px dashed var(--edge);
  font-size: .55em;
  color: var(--ink-dim);
  white-space: pre-wrap;
}
.page-number { position: absolute; right: 3vw; bottom: 2vh; font-size: 12px; color: var(--ink-dim); }
.page { position: relative; }
@page { size: landscape; margin: 0; }
"#;

pub fn render(handout: &Handout) -> String {
    let slides = deck::parse(handout.markdown);
    let look = deck::theme_of(handout.markdown);
    let base = Web::get("base.css")
        .and_then(|f| String::from_utf8(f.data.to_vec()).ok())
        .unwrap_or_default();
    let theme_css = look
        .as_ref()
        .and_then(|l| handout.styles.theme(&l.name))
        .map(|sheet| sheet.css.clone())
        .unwrap_or_default();
    let knobs = look
        .as_ref()
        .map(|l| {
            l.knobs
                .iter()
                .map(|(k, v)| format!("--knob-{k}: {v};"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let title = slides
        .first()
        .map(|s| crate::export::plain(&s.html))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Deck".to_string());

    let mut out = String::with_capacity(base.len() + theme_css.len() + slides.len() * 512);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>{}</title>\n<style>\n", escape(&title)));
    out.push_str(&base);
    out.push('\n');
    out.push_str(&theme_css);
    out.push_str(PAGE_CSS);
    out.push_str("</style>\n</head>\n");
    out.push_str(&format!(
        "<body class=\"viewer handout\" style=\"{}\">\n",
        escape(&knobs)
    ));
    let total = slides.len();
    for (index, slide) in slides.iter().enumerate() {
        out.push_str("<section class=\"page\">\n<div class=\"slide\">");
        out.push_str(&inline_images(&slide.html, handout));
        out.push_str("</div>\n");
        out.push_str(&asked(slide, handout.with_notes));
        if handout.with_notes && !slide.notes.is_empty() {
            out.push_str(&format!(
                "<aside class=\"handout-notes\">{}</aside>\n",
                escape(&slide.notes)
            ));
        }
        out.push_str(&format!(
            "<span class=\"page-number\">{} / {total}</span>\n</section>\n",
            index + 1
        ));
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// The question or poll a slide asks, as a list the page can print. Right
/// answers are marked only on the copy that carries the notes.
fn asked(slide: &Slide, with_notes: bool) -> String {
    if let Some(question) = &slide.question {
        let mut out = String::from("<ul class=\"handout-options\">\n");
        for (index, option) in question.options.iter().enumerate() {
            let right = with_notes && question.correct.contains(&index);
            out.push_str(&format!(
                "<li{}>{}{}</li>\n",
                if right { " class=\"right\"" } else { "" },
                if right { "\u{2713} " } else { "" },
                escape(option)
            ));
        }
        out.push_str("</ul>\n");
        return out;
    }
    if let Some(poll) = &slide.poll {
        let label = match poll {
            Poll::Text => "Poll: a word from everyone".to_string(),
            Poll::Scale { min, max } => format!("Poll: {min} to {max}"),
            Poll::Rating { max } => format!("Poll: {max} stars"),
        };
        return format!("<p class=\"handout-poll\">{}</p>\n", escape(&label));
    }
    String::new()
}

/// Swaps the room's own picture links for the bytes, so the file keeps them.
fn inline_images(html: &str, handout: &Handout) -> String {
    let Some(session) = handout.session else {
        return html.to_string();
    };
    let mut out = html.to_string();
    for image in handout.images {
        let link = format!("/i/{session}/{}", image.id);
        if out.contains(&link) {
            let data = format!(
                "data:{};base64,{}",
                image.kind,
                base64::engine::general_purpose::STANDARD.encode(&image.bytes)
            );
            out = out.replace(&link, &data);
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles() -> Styles {
        crate::styles::load(None, None).unwrap()
    }

    fn render_plain(markdown: &str, with_notes: bool) -> String {
        render(&Handout {
            markdown,
            styles: &styles(),
            session: None,
            images: &[],
            with_notes,
        })
    }

    const DECK: &str = "<!-- theme: paper -->\n# One\n\n???\nsay hi\n\n---\n\n# Q\n\n- [ ] a\n- [x] b\n\n---\n\n<!-- poll: rating 5 -->\n# Stars";

    #[test]
    fn every_slide_is_a_page_and_the_theme_rides_along() {
        let html = render_plain(DECK, false);
        assert_eq!(html.matches("<section class=\"page\">").count(), 3);
        assert!(html.contains("<title>One</title>"), "{html}");
        assert!(html.contains("--ground:"), "base.css is missing");
        assert!(html.contains("Georgia"), "the paper theme is missing");
        assert!(html.contains("3 / 3"));
    }

    #[test]
    fn notes_and_right_answers_appear_only_when_asked_for() {
        let bare = render_plain(DECK, false);
        assert!(!bare.contains("say hi"));
        assert!(!bare.contains("class=\"right\""));
        assert!(bare.contains("<li>b</li>"));
        let full = render_plain(DECK, true);
        assert!(full.contains("say hi"));
        assert!(
            full.contains("<li class=\"right\">\u{2713} b</li>"),
            "{full}"
        );
        assert!(full.contains("Poll: 5 stars"));
    }

    #[test]
    fn notes_are_text_and_never_markup() {
        let html = render_plain("# One\n\n???\n<script>alert(1)</script>", true);
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_room_s_pictures_travel_as_bytes() {
        let image = Stored {
            id: "pic1".into(),
            kind: "image/png",
            bytes: vec![1, 2, 3],
        };
        let html = render(&Handout {
            markdown: "# P\n\n![a crab](/i/room7/pic1)",
            styles: &styles(),
            session: Some("room7"),
            images: std::slice::from_ref(&image),
            with_notes: false,
        });
        assert!(
            html.contains("src=\"data:image/png;base64,AQID\""),
            "{html}"
        );
        assert!(!html.contains("/i/room7/"));
    }

    #[test]
    fn a_deck_with_no_theme_still_renders() {
        let html = render_plain("# Plain", false);
        assert!(
            html.contains("<body class=\"viewer handout\" style=\"\">"),
            "{html}"
        );
    }
}
