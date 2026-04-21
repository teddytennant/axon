//! Small shared utilities used across the workspace.
//!
//! Everything here exists because the same 3–5 line snippet was copy-pasted
//! across half a dozen modules. Keep this module strictly limited to things
//! that are genuinely identical and very small — anything bigger belongs in
//! its own module.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the UNIX epoch. Returns 0 if the system clock is set before
/// 1970 (which is treated as a "no-data" sentinel by callers).
#[inline]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Milliseconds since the UNIX epoch. Same fallback semantics as [`now_secs`].
#[inline]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Lowercase hex encoding of a byte slice.
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Decode a lowercase or uppercase hex string into bytes. Invalid characters,
/// odd-length input, or non-ASCII strings are silently dropped — callers that
/// want strict parsing should use the `hex` crate directly.
pub fn hex_decode(s: &str) -> Vec<u8> {
    if !s.is_ascii() {
        return Vec::new();
    }
    let len = s.len() & !1; // round down to even
    (0..len)
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// First `n` bytes of a peer ID as lowercase hex. Used in log lines and
/// dashboard rendering. The convention is `n = 4` (8 hex chars).
pub fn peer_short(id: &[u8], n: usize) -> String {
    hex_encode(&id[..id.len().min(n)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_secs_is_after_2020() {
        assert!(now_secs() > 1_577_836_800);
    }

    #[test]
    fn now_ms_is_larger_than_now_secs() {
        let s = now_secs();
        let ms = now_ms();
        assert!(ms / 1000 >= s);
    }

    #[test]
    fn hex_encode_roundtrip() {
        let bytes = [0x00, 0x01, 0x10, 0xff, 0xab];
        let s = hex_encode(&bytes);
        assert_eq!(s, "000110ffab");
        assert_eq!(hex_decode(&s), bytes.to_vec());
    }

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_decode(""), Vec::<u8>::new());
    }

    #[test]
    fn hex_decode_odd_length() {
        assert_eq!(hex_decode("abc"), vec![0xab]);
    }

    #[test]
    fn hex_decode_non_ascii_returns_empty() {
        assert_eq!(hex_decode("ⓍⓍ"), Vec::<u8>::new());
    }

    #[test]
    fn hex_decode_invalid_chars() {
        assert_eq!(hex_decode("zz"), Vec::<u8>::new());
    }

    #[test]
    fn peer_short_truncates() {
        let id = vec![0xde, 0xad, 0xbe, 0xef, 0x00, 0x11];
        assert_eq!(peer_short(&id, 4), "deadbeef");
        assert_eq!(peer_short(&id, 2), "dead");
    }

    #[test]
    fn peer_short_handles_short_input() {
        let id = vec![0x12, 0x34];
        assert_eq!(peer_short(&id, 4), "1234");
    }
}
