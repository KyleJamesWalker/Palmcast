use std::sync::OnceLock;

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/"]
#[exclude = "*.test.mjs"]
pub struct Web;

/// One id for the whole asset set, derived from the files themselves.
///
/// Revalidation alone is not enough: a browser holding an entry cached by an
/// earlier build that sent no validator can keep serving it. Putting the id in
/// every asset URL means a new build asks for URLs the old cache has never
/// seen, so stale code cannot survive a redeploy.
pub fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let mut names: Vec<String> = Web::iter().map(|f| f.to_string()).collect();
        names.sort();
        let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
        for name in names {
            let Some(file) = Web::get(&name) else {
                continue;
            };
            for byte in name.as_bytes().iter().chain(file.data.iter()) {
                acc ^= *byte as u64;
                acc = acc.wrapping_mul(0x1000_0000_01b3);
            }
        }
        format!("{acc:016x}")
    })
}

/// Rewrites root relative asset references to carry the build id. Applied to
/// html and js, because a module import is a reference the browser caches too.
pub fn versioned(body: &str) -> String {
    let id = build_id();
    let mut out = String::with_capacity(body.len() + 64);
    let mut rest = body;

    while let Some(start) = rest.find("\"/").or_else(|| rest.find("'/")) {
        let quote = rest.as_bytes()[start] as char;
        let after = &rest[start + 2..];
        let Some(end) = after.find(quote) else { break };
        let path = &after[..end];

        out.push_str(&rest[..start + 2]);
        out.push_str(path);
        if (path.ends_with(".js") || path.ends_with(".css")) && !path.contains('?') {
            out.push_str("?v=");
            out.push_str(id);
        }
        out.push(quote);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}
