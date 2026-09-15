//! The looks an instance can serve.
//!
//! Themes and transitions are ordinary css files keyed by their own file name,
//! whether they ship in the binary or the operator dropped them in a directory
//! at startup. A deck names one; it never carries one, because anyone with the
//! link may write a deck.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::assets::Web;
use crate::deck::style_name;

/// A look, as a page offering it needs to describe it: what a deck writes, and
/// what the file says it looks like.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Look {
    pub name: String,
    /// The file's own opening comment, to a sentence or two. Empty when the
    /// file has none, because an operator is not obliged to explain theirs.
    pub about: String,
    /// What a deck may change about it, with the value the file itself uses as
    /// the default. Empty for a look that declares none, which is every look
    /// written before there were any.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub knobs: Vec<Knob>,
}

/// One custom property a look put its name to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Knob {
    pub name: String,
    /// The value the stylesheet falls back to, which is what a picker should
    /// open on.
    pub value: String,
}

pub struct Sheet {
    pub css: String,
    pub knobs: Vec<Knob>,
    /// Quoted, ready for the header. Over the bytes themselves, so an operator
    /// who edits a file and restarts gets it past every cache.
    pub etag: String,
    pub about: String,
}

/// `Default` is an instance that ships nothing, which is not what any real one
/// looks like. It exists so a caller that cannot read the built-ins still has
/// something to hand the router rather than refusing to start.
#[derive(Default)]
pub struct Styles {
    themes: BTreeMap<String, Sheet>,
    transitions: BTreeMap<String, Sheet>,
}

impl Styles {
    pub fn theme(&self, name: &str) -> Option<&Sheet> {
        self.themes.get(name)
    }

    pub fn transition(&self, name: &str) -> Option<&Sheet> {
        self.transitions.get(name)
    }

    /// Every theme this instance serves, named and described. The order is the
    /// map's, which is alphabetical, so a picker built from it is too.
    pub fn themes(&self) -> Vec<Look> {
        looks(&self.themes)
    }

    pub fn transitions(&self) -> Vec<Look> {
        looks(&self.transitions)
    }

    pub fn theme_names(&self) -> Vec<String> {
        self.themes.keys().cloned().collect()
    }

    pub fn transition_names(&self) -> Vec<String> {
        self.transitions.keys().cloned().collect()
    }
}

fn looks(from: &BTreeMap<String, Sheet>) -> Vec<Look> {
    from.iter()
        .map(|(name, sheet)| Look {
            name: name.clone(),
            about: sheet.about.clone(),
            knobs: sheet.knobs.clone(),
        })
        .collect()
}

/// Built-ins first, then whatever the operator named on top of them, so an
/// instance can replace a look it ships with rather than only add to it.
pub fn load(themes: Option<&Path>, transitions: Option<&Path>) -> Result<Styles, String> {
    let mut styles = Styles {
        themes: built_in("themes/"),
        transitions: built_in("transitions/"),
    };
    if let Some(dir) = themes {
        merge(&mut styles.themes, dir)?;
    }
    if let Some(dir) = transitions {
        merge(&mut styles.transitions, dir)?;
    }
    Ok(styles)
}

fn built_in(prefix: &str) -> BTreeMap<String, Sheet> {
    let mut out = BTreeMap::new();
    for path in Web::iter() {
        let Some(name) = path
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".css"))
            .and_then(style_name)
        else {
            continue;
        };
        let Some(file) = Web::get(&path) else {
            continue;
        };
        let Ok(css) = String::from_utf8(file.data.to_vec()) else {
            continue;
        };
        out.insert(name, sheet(css));
    }
    out
}

/// A directory the operator named and the server cannot read stops startup, on
/// the same reasoning as a named deck it cannot open: the first visitor of the
/// evening is too late to find out the looks never loaded.
fn merge(into: &mut BTreeMap<String, Sheet>, dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("could not read {}: {error}", dir.display()))?;

    for entry in entries {
        let path = entry
            .map_err(|error| format!("could not read {}: {error}", dir.display()))?
            .path();
        let Some(stem) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".css"))
        else {
            continue;
        };
        let Some(name) = style_name(stem) else {
            tracing::warn!(
                "skipping {}: a name is lowercase letters, digits and dashes",
                path.display()
            );
            continue;
        };
        let css = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        into.insert(name, sheet(css));
    }
    Ok(())
}

fn sheet(css: String) -> Sheet {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in css.as_bytes() {
        acc ^= *byte as u64;
        acc = acc.wrapping_mul(0x1000_0000_01b3);
    }
    Sheet {
        etag: format!("\"{acc:016x}\""),
        about: about(&css),
        knobs: knobs(&css),
        css,
    }
}

/// The knobs a look put its name to, as `--knob-<name>: <value>;`.
///
/// Read out of the stylesheet rather than a manifest beside it, the same way
/// `about` is, so the names and their defaults cannot drift apart from the
/// rules that use them. A file declaring none is a file nothing changes about.
fn knobs(css: &str) -> Vec<Knob> {
    let mut found: Vec<Knob> = Vec::new();
    for (at, _) in css.match_indices("--knob-") {
        let rest = &css[at + "--knob-".len()..];
        let Some((name, rest)) = rest.split_once(':') else {
            continue;
        };
        // A declaration, not a `var(--knob-x)` reading one back.
        let Some(name) = style_name(name) else {
            continue;
        };
        let value = rest.split([';', '}']).next().unwrap_or_default().trim();
        if value.is_empty() || value.len() > 64 || value.contains("var(") {
            continue;
        }
        if found.iter().any(|knob| knob.name == name) {
            continue;
        }
        found.push(Knob {
            name,
            value: value.to_string(),
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// What a file says about itself, for a picker to put next to the name.
///
/// The opening comment, wrapped back onto one line and cut after a sentence or
/// two. Every built-in starts with one, and an operator who writes one gets the
/// same treatment for free rather than having to fill in a manifest.
fn about(css: &str) -> String {
    let Some(comment) = css.trim_start().strip_prefix("/*").and_then(|rest| {
        rest.split_once("*/")
            .map(|(comment, _)| comment)
            .filter(|_| !css.trim_start().starts_with("/**/"))
    }) else {
        return String::new();
    };

    let flat = comment.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    // Sentences until there is enough to be worth reading, because a look whose
    // first sentence is "A bar at midnight." has not said anything yet.
    for sentence in flat.split_inclusive(". ") {
        if !out.is_empty() && (out.len() >= 40 || out.len() + sentence.len() > 140) {
            break;
        }
        out.push_str(sentence);
    }
    if out.is_empty() {
        out = flat;
    }
    out.truncate(140);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pc-style-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_look_declares_its_own_knobs_and_most_declare_none() {
        let looks = load(None, None).unwrap();

        let neon = looks
            .themes()
            .into_iter()
            .find(|l| l.name == "neon")
            .expect("no neon");
        assert_eq!(
            neon.knobs,
            vec![
                Knob {
                    name: "accent".into(),
                    value: "#ff3ea5".into()
                },
                Knob {
                    name: "heading".into(),
                    value: "#3ef0ff".into()
                },
            ],
            "neon did not surface the knobs its stylesheet declares"
        );

        // Every other built-in declares none, and is what it always was.
        for look in looks.themes().into_iter().filter(|l| l.name != "neon") {
            assert!(
                look.knobs.is_empty(),
                "{} surfaced knobs it does not declare",
                look.name
            );
        }
    }

    #[test]
    fn reading_a_knob_back_is_not_declaring_one() {
        // `var(--knob-x)` is a use, and a use is not a declaration.
        let found = knobs(".a { color: var(--knob-heading); --knob-real: #fff; }");
        assert_eq!(
            found,
            vec![Knob {
                name: "real".into(),
                value: "#fff".into()
            }],
            "a var() reading a knob was taken for a declaration"
        );
    }

    #[test]
    fn the_built_in_looks_are_there_without_an_operator_saying_anything() {
        let styles = load(None, None).unwrap();
        assert!(
            styles.theme("ember").is_some(),
            "the default theme is missing"
        );
        assert!(styles.transition("fade").is_some(), "fade is missing");
        assert!(styles.theme_names().contains(&"paper".to_string()));
    }

    #[test]
    fn a_directory_the_operator_named_adds_to_them() {
        let dir = scratch("add");
        std::fs::write(dir.join("midnight.css"), ":root { --ground: #000; }").unwrap();

        let styles = load(Some(&dir), None).unwrap();
        assert_eq!(
            styles.theme("midnight").map(|s| s.css.as_str()),
            Some(":root { --ground: #000; }")
        );
        assert!(styles.theme("ember").is_some(), "the built ins went away");
    }

    #[test]
    fn an_operator_file_replaces_a_built_in_of_the_same_name() {
        let dir = scratch("replace");
        std::fs::write(dir.join("ember.css"), "/* mine */").unwrap();

        let styles = load(Some(&dir), None).unwrap();
        assert_eq!(
            styles.theme("ember").map(|s| s.css.as_str()),
            Some("/* mine */")
        );
        assert_eq!(
            styles
                .theme_names()
                .iter()
                .filter(|n| *n == "ember")
                .count(),
            1,
            "the name is listed twice"
        );
    }

    #[test]
    fn a_file_name_a_deck_could_not_ask_for_is_left_out() {
        let dir = scratch("names");
        std::fs::write(dir.join("Loud.css"), "x").unwrap();
        std::fs::write(dir.join("with space.css"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();

        let styles = load(Some(&dir), None).unwrap();
        for absent in ["Loud", "loud", "with space", "notes"] {
            assert!(styles.theme(absent).is_none(), "took {absent}");
        }
    }

    #[test]
    fn a_directory_the_operator_named_and_the_server_cannot_read_stops_it() {
        let missing = std::env::temp_dir().join("palmcast-no-such-theme-dir");
        std::fs::remove_dir_all(&missing).ok();
        assert!(load(Some(&missing), None).is_err());
    }

    #[test]
    fn transitions_load_from_their_own_directory() {
        let dir = scratch("trans");
        std::fs::write(dir.join("swoosh.css"), "@keyframes x {}").unwrap();

        let styles = load(None, Some(&dir)).unwrap();
        assert!(styles.transition("swoosh").is_some());
        assert!(
            styles.theme("swoosh").is_none(),
            "a transition became a theme"
        );
    }

    #[test]
    fn two_sheets_that_differ_do_not_share_an_etag() {
        let dir = scratch("etag");
        std::fs::write(dir.join("one.css"), "a{}").unwrap();
        std::fs::write(dir.join("two.css"), "b{}").unwrap();

        let styles = load(Some(&dir), None).unwrap();
        assert_ne!(
            styles.theme("one").unwrap().etag,
            styles.theme("two").unwrap().etag
        );
    }

    /// A transparent reading surface is two slides of text at once, which is
    /// invisible until a transition puts one over the other. The rule lives in
    /// base.css for the unthemed case, and every theme has to keep it: a theme
    /// that sets the ink and not the ground leaves the surface see through.
    #[test]
    fn the_reading_surface_is_never_transparent() {
        let base = String::from_utf8(Web::get("base.css").unwrap().data.to_vec()).unwrap();
        let block = base
            .split_once(".viewer, .stage {")
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(block, _)| block.to_string())
            .expect("base.css no longer styles the surface as one rule");
        assert!(
            block.contains("background:"),
            "base.css leaves the surface transparent: {block}"
        );

        let styles = load(None, None).unwrap();
        for name in styles.theme_names() {
            let css = &styles.theme(&name).unwrap().css;
            assert!(
                css.contains("background:"),
                "the {name} theme paints no ground"
            );
        }
    }

    #[test]
    fn a_look_describes_itself_from_its_own_opening_comment() {
        let styles = load(None, None).unwrap();
        let cover = styles
            .transitions()
            .into_iter()
            .find(|l| l.name == "cover")
            .unwrap();
        assert_eq!(
            cover.about,
            "The next slide rises from the bottom over a slide that stays put."
        );

        // Two sentences where the first is too short to say much on its own.
        let neon = styles
            .themes()
            .into_iter()
            .find(|l| l.name == "neon")
            .unwrap();
        assert!(
            neon.about.starts_with("A bar at midnight."),
            "{}",
            neon.about
        );
        assert!(neon.about.contains("cyan"), "{}", neon.about);
    }

    #[test]
    fn every_look_this_instance_serves_says_what_it_is() {
        let styles = load(None, None).unwrap();
        for look in styles.themes().into_iter().chain(styles.transitions()) {
            assert!(
                look.about.len() > 12,
                "{} says nothing useful: {:?}",
                look.name,
                look.about
            );
            assert!(
                !look.about.contains("/*"),
                "{} kept its comment markers",
                look.name
            );
            assert!(!look.about.contains('\n'), "{} spans lines", look.name);
        }
    }

    #[test]
    fn a_file_with_no_comment_is_still_offered() {
        let dir = scratch("nocomment");
        std::fs::write(dir.join("bare.css"), ".viewer { background: #000; }").unwrap();

        let styles = load(Some(&dir), None).unwrap();
        let bare = styles
            .themes()
            .into_iter()
            .find(|l| l.name == "bare")
            .unwrap();
        assert_eq!(bare.about, "");
    }
}
