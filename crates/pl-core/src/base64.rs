//! Base64, per RFC 4648 — the standard alphabet and the URL-safe one.
//!
//! Encoding only; nothing here needs to decode. Hand-written for the same
//! reason as [`crate::sha1`]: `pl-core` takes no dependencies, and this feeds
//! the checksum that every correctness claim in the project rests on.

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_with(alphabet: &[u8; 64], data: &[u8], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(alphabet[(n >> 18 & 63) as usize] as char);
        out.push(alphabet[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(alphabet[(n >> 6 & 63) as usize] as char);
        } else if pad {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(alphabet[(n & 63) as usize] as char);
        } else if pad {
            out.push('=');
        }
    }
    out
}

/// Standard base64 (`+` and `/`), without padding.
pub fn encode_standard_nopad(data: &[u8]) -> String {
    encode_with(STANDARD, data, false)
}

/// URL-safe base64 (`-` and `_`), without padding.
pub fn encode_urlsafe_nopad(data: &[u8]) -> String {
    encode_with(URL_SAFE, data, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vectors_without_padding() {
        // RFC 4648 section 10, with the '=' padding stripped.
        for (input, expected) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(
                encode_standard_nopad(input.as_bytes()),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn padding_is_emitted_when_asked_for() {
        assert_eq!(encode_with(STANDARD, b"f", true), "Zg==");
        assert_eq!(encode_with(STANDARD, b"fo", true), "Zm8=");
        assert_eq!(encode_with(STANDARD, b"foo", true), "Zm9v");
    }

    #[test]
    fn the_two_alphabets_differ_only_in_the_last_two_symbols() {
        // Bytes chosen to produce indices 62 and 63.
        let data = [0xFBu8, 0xFF, 0xBF];
        let s = encode_standard_nopad(&data);
        let u = encode_urlsafe_nopad(&data);
        assert!(s.contains('+') && s.contains('/'), "{s}");
        assert!(u.contains('-') && u.contains('_'), "{u}");
        assert_eq!(s.replace('+', "-").replace('/', "_"), u);
    }

    #[test]
    fn a_sha1_digest_encodes_to_27_characters() {
        // 20 bytes -> ceil(20/3)*4 = 28 with padding, 27 without.
        let d = crate::sha1::sha1(b"GATTACA");
        assert_eq!(encode_urlsafe_nopad(&d).len(), 27);
        assert_eq!(encode_standard_nopad(&d).len(), 27);
    }
}
