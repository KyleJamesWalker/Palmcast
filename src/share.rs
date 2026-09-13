//! Packs a deck into a link you can paste into a chat.
//!
//! MessagePack for the envelope, Brotli for the squeeze, base64url for the
//! trip through a URL. The token is the whole deck: no room, no row in a
//! table, nothing to expire. Anyone holding the link can rebuild the deck
//! offline, which is the point.

use std::io::{Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};

/// Bumped only when an old build would misread a new token. Readers refuse a
/// version they do not know rather than guess at the fields.
const VERSION: u8 = 1;

/// Chat clients, not browsers, set the ceiling on a pasted link. Brotli gets
/// roughly 8:1 on markdown, so this still carries a deck far longer than
/// anyone writes between rounds.
pub const MAX_TOKEN_CHARS: usize = 8192;

/// Packing is the one unauthenticated request that does real work: quality 11
/// on a deck at the 256 KiB create limit costs 124ms of CPU. Refusing above
/// 64 KiB caps that at 28ms and still accepts twenty times the longest deck
/// anyone writes between rounds.
pub const MAX_SHARE_BYTES: usize = 64 * 1024;

/// Matches the deck limit the create endpoint enforces, so a token can never
/// smuggle in a deck the server would have refused. Also the bomb guard: the
/// decoder stops reading here instead of trusting the compressed size.
const MAX_MARKDOWN_BYTES: usize = 256 * 1024;

const QUALITY: u32 = 11;
const WINDOW: u32 = 22;

/// Serialized as a MessagePack array, so a later version appends fields and an
/// older reader still finds the version in slot zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Envelope {
    v: u8,
    markdown: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PackError {
    /// Too long before compressing, or still too long after.
    TooLong,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UnpackError {
    TooLong,
    NotBase64,
    NotBrotli,
    /// Decompressed past the deck limit, or the bytes are not an envelope.
    Malformed,
    /// A token from a newer build.
    Version(u8),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::TooLong => f.write_str("this deck is too long to share as a link"),
        }
    }
}

impl std::fmt::Display for UnpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            UnpackError::TooLong => "that link is too long to be a Palmcast deck",
            UnpackError::NotBase64 => "that link is not a Palmcast deck",
            UnpackError::NotBrotli | UnpackError::Malformed => "that deck link is damaged",
            UnpackError::Version(_) => "that link needs a newer version of Palmcast",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PackError {}
impl std::error::Error for UnpackError {}

pub fn pack(markdown: &str) -> Result<String, PackError> {
    if markdown.len() > MAX_SHARE_BYTES {
        return Err(PackError::TooLong);
    }
    let envelope = Envelope {
        v: VERSION,
        markdown: markdown.to_string(),
    };
    // Infallible: the envelope is two owned fields with no map keys to reject.
    let packed = rmp_serde::to_vec(&envelope).expect("envelope is serializable");

    let mut squeezed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut squeezed, 4096, QUALITY, WINDOW);
        writer.write_all(&packed).expect("writing to a Vec");
        writer.flush().expect("writing to a Vec");
    }

    let token = B64.encode(&squeezed);
    if token.len() > MAX_TOKEN_CHARS {
        return Err(PackError::TooLong);
    }
    Ok(token)
}

pub fn unpack(token: &str) -> Result<String, UnpackError> {
    if token.len() > MAX_TOKEN_CHARS {
        return Err(UnpackError::TooLong);
    }
    let squeezed = B64.decode(token).map_err(|_| UnpackError::NotBase64)?;

    // `take` is the whole bomb guard: a few hundred bytes of Brotli can ask for
    // gigabytes, and the reader stops one byte past the limit either way.
    let mut packed = Vec::new();
    brotli::Decompressor::new(squeezed.as_slice(), 4096)
        .take(MAX_MARKDOWN_BYTES as u64 + 1)
        .read_to_end(&mut packed)
        .map_err(|_| UnpackError::NotBrotli)?;
    if packed.len() > MAX_MARKDOWN_BYTES {
        return Err(UnpackError::Malformed);
    }

    let envelope: Envelope = rmp_serde::from_slice(&packed).map_err(|_| UnpackError::Malformed)?;
    if envelope.v != VERSION {
        return Err(UnpackError::Version(envelope.v));
    }
    if envelope.markdown.len() > MAX_MARKDOWN_BYTES {
        return Err(UnpackError::Malformed);
    }
    Ok(envelope.markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK: &str = "# Why Rust\n\n---\n\n## The pitch\n\n- No GC\n- No data races\n";

    #[test]
    fn a_deck_survives_the_round_trip() {
        let token = pack(DECK).unwrap();
        assert_eq!(unpack(&token).unwrap(), DECK);
    }

    #[test]
    fn a_token_is_safe_in_a_url() {
        let token = pack(DECK).unwrap();
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token carried a character a chat client would break: {token}"
        );
    }

    #[test]
    fn unicode_and_newlines_come_back_byte_for_byte() {
        let deck = "# Café ☕\r\n\r\n---\r\n\r\n- [x] naïve\n- [ ] 日本語\n\ttab\n";
        assert_eq!(unpack(&pack(deck).unwrap()).unwrap(), deck);
    }

    #[test]
    fn an_empty_deck_round_trips() {
        assert_eq!(unpack(&pack("").unwrap()).unwrap(), "");
    }

    #[test]
    fn a_quiz_deck_gets_smaller_not_bigger() {
        let deck = DECK.repeat(40);
        let token = pack(&deck).unwrap();
        assert!(
            token.len() < deck.len() / 4,
            "{} characters for a {} byte deck",
            token.len(),
            deck.len()
        );
    }

    #[test]
    fn a_deck_over_the_share_limit_is_refused_before_it_is_compressed() {
        // All one byte: Brotli would fold this to nothing, so only the length
        // check can be what refuses it.
        let deck = "a".repeat(MAX_SHARE_BYTES + 1);
        assert_eq!(pack(&deck), Err(PackError::TooLong));
        assert!(pack(&"a".repeat(MAX_SHARE_BYTES)).is_ok());
    }

    #[test]
    fn a_deck_that_will_not_squeeze_small_enough_is_refused() {
        // Incompressible bytes inside the input limit: the token overruns.
        let mut deck = String::new();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        while deck.len() < MAX_SHARE_BYTES {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            deck.push_str(&format!("{seed:016x}"));
        }
        assert_eq!(pack(&deck), Err(PackError::TooLong));
    }

    #[test]
    fn junk_is_refused_rather_than_decoded() {
        assert_eq!(unpack("not a token!!"), Err(UnpackError::NotBase64));
        assert_eq!(
            unpack(&B64.encode(b"still not brotli")),
            Err(UnpackError::NotBrotli)
        );
        assert_eq!(
            unpack(&"A".repeat(MAX_TOKEN_CHARS + 1)),
            Err(UnpackError::TooLong)
        );
    }

    #[test]
    fn brotli_that_is_not_an_envelope_is_malformed() {
        let mut squeezed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut squeezed, 4096, QUALITY, WINDOW);
            writer.write_all(b"plain bytes, no messagepack").unwrap();
        }
        assert_eq!(unpack(&B64.encode(&squeezed)), Err(UnpackError::Malformed));
    }

    #[test]
    fn a_future_version_is_named_not_guessed_at() {
        let envelope = Envelope {
            v: VERSION + 1,
            markdown: "from the future".into(),
        };
        let packed = rmp_serde::to_vec(&envelope).unwrap();
        let mut squeezed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut squeezed, 4096, QUALITY, WINDOW);
            writer.write_all(&packed).unwrap();
        }
        assert_eq!(
            unpack(&B64.encode(&squeezed)),
            Err(UnpackError::Version(VERSION + 1))
        );
    }

    #[test]
    fn a_decompression_bomb_is_refused_not_allocated() {
        // Half a megabyte of zeros squeezes to a few hundred bytes, so the
        // token sails past every length check before it is decompressed.
        let bomb = vec![b'a'; MAX_MARKDOWN_BYTES * 2];
        let mut squeezed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut squeezed, 4096, QUALITY, WINDOW);
            writer.write_all(&bomb).unwrap();
        }
        let token = B64.encode(&squeezed);
        assert!(
            token.len() < MAX_TOKEN_CHARS,
            "the bomb must reach the decoder"
        );
        assert_eq!(unpack(&token), Err(UnpackError::Malformed));
    }
}
