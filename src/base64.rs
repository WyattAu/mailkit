//! Internal base64 (RFC 4648 standard alphabet) encoder.
//!
//! Hand-rolled to keep the dependency tree light: only encoding is needed
//! (MIME transfer-encoding, JSON attachment payloads), never decoding.
//! Includes a streaming wrapper encoder that carries sub-3-byte remainders
//! across chunk boundaries and folds output at 76 columns with CRLF, as
//! required for MIME `Content-Transfer-Encoding: base64` bodies.

use std::time::{SystemTime, UNIX_EPOCH};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PAD: u8 = b'=';
/// MIME base64 bodies are folded at 76 characters (RFC 2045).
const LINE_WIDTH: usize = 76;

/// Encode `data` as standard base64 with padding, without line folding.
pub(crate) fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for t in &mut chunks {
        push_triple(&mut out, t[0], t[1], t[2]);
    }
    match *chunks.remainder() {
        [a] => {
            out.push(ALPHABET[(a >> 2) as usize] as char);
            out.push(ALPHABET[((a & 0x03) << 4) as usize] as char);
            out.push(PAD as char);
            out.push(PAD as char);
        }
        [a, b] => {
            out.push(ALPHABET[(a >> 2) as usize] as char);
            out.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
            out.push(ALPHABET[((b & 0x0F) << 2) as usize] as char);
            out.push(PAD as char);
        }
        _ => {}
    }
    out
}

fn push_triple(out: &mut String, a: u8, b: u8, c: u8) {
    let n = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
    out.push(ALPHABET[(n >> 18) as usize & 0x3F] as char);
    out.push(ALPHABET[(n >> 12) as usize & 0x3F] as char);
    out.push(ALPHABET[(n >> 6) as usize & 0x3F] as char);
    out.push(ALPHABET[n as usize & 0x3F] as char);
}

/// Streaming base64 encoder that folds output at 76 columns with CRLF.
///
/// Accepts arbitrary chunk sizes: sub-triple remainders are carried across
/// `push` calls so no mid-stream padding is ever emitted. `finish` encodes
/// the final remainder with padding and returns the full folded string.
pub(crate) struct FoldingEncoder {
    out: String,
    col: usize,
    carry: Vec<u8>,
}

impl FoldingEncoder {
    pub(crate) fn new() -> Self {
        Self {
            out: String::new(),
            col: 0,
            carry: Vec::with_capacity(2),
        }
    }

    /// Feed the next chunk of raw bytes (any size).
    pub(crate) fn push(&mut self, data: &[u8]) {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(data);
        let full = buf.len() - buf.len() % 3;
        for t in buf[..full].chunks_exact(3) {
            let n = (u32::from(t[0]) << 16) | (u32::from(t[1]) << 8) | u32::from(t[2]);
            self.put(((n >> 18) & 0x3F) as usize);
            self.put(((n >> 12) & 0x3F) as usize);
            self.put(((n >> 6) & 0x3F) as usize);
            self.put((n & 0x3F) as usize);
        }
        self.carry = buf[full..].to_vec();
    }

    /// Encode the final sub-triple remainder (with padding) and return the
    /// folded base64 output.
    pub(crate) fn finish(mut self) -> String {
        match std::mem::take(&mut self.carry)[..] {
            [] => {}
            [a] => {
                self.put((a >> 2) as usize);
                self.put(((a & 0x03) << 4) as usize);
                self.pad(2);
                self.out.push_str("==");
            }
            [a, b] => {
                self.put((a >> 2) as usize);
                self.put((((a & 0x03) << 4) | (b >> 4)) as usize);
                self.put(((b & 0x0F) << 2) as usize);
                self.pad(1);
                self.out.push(PAD as char);
            }
            _ => unreachable!("carry is always shorter than 3 bytes"),
        }
        self.out
    }

    /// Break the line first if `n` padding characters would overflow it.
    fn pad(&mut self, n: usize) {
        if self.col + n > LINE_WIDTH {
            self.out.push_str("\r\n");
            self.col = 0;
        }
        self.col += n;
    }

    fn put(&mut self, sextet: usize) {
        if self.col == LINE_WIDTH {
            self.out.push_str("\r\n");
            self.col = 0;
        }
        self.out.push(ALPHABET[sextet] as char);
        self.col += 1;
    }
}

/// Generate a collision-resistant hex string for MIME boundaries and
/// message ids: SplitMix64 mixed over an atomic counter and wall-clock nanos.
pub(crate) fn random_hex(len: usize) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    use std::sync::atomic::Ordering;
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut z = counter.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ nanos.rotate_left(17);
    let mix = |z: &mut u64| {
        *z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut t = *z;
        t = (t ^ (t >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        t = (t ^ (t >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        t ^ (t >> 31)
    };
    let a = mix(&mut z);
    let b = mix(&mut z);
    format!("{a:016x}{b:016x}")[..len].to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// RFC 4648 §10 test vectors.
    #[test]
    fn rfc4648_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encode_binary_and_long_input() {
        // Multiple of 3: no padding.
        let data: Vec<u8> = (0..=255u8).cycle().take(9_999).collect();
        let encoded = encode(&data);
        assert_eq!(encoded.len(), data.len().div_ceil(3) * 4);
        assert!(!encoded.contains('='));
        // One byte over a multiple of 3: exactly one padded quad.
        assert!(encode(&data[..1]).ends_with("=="));
        assert!(encode(&data[..2]).ends_with('='));
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
        );
    }

    #[test]
    fn folding_encoder_matches_unfolded_encode() {
        for len in [0usize, 1, 2, 3, 4, 5, 25, 57, 76, 77, 1000] {
            let data: Vec<u8> = (0..len as u8).collect();
            let plain = encode(&data);
            let mut enc = FoldingEncoder::new();
            // Push in awkward chunk sizes to exercise carry logic.
            for chunk in data.chunks(7) {
                enc.push(chunk);
            }
            let folded = enc.finish();
            let expected = fold(&plain);
            assert_eq!(folded, expected, "mismatch at len {len}");
            assert!(
                !folded
                    .split("\r\n")
                    .any(|l| l.len() > 76 && !l.ends_with('='))
            );
        }
    }

    #[test]
    fn folding_encoder_lines_within_76() {
        let data: Vec<u8> = (0..=255u8).cycle().take(30_000).collect();
        let mut enc = FoldingEncoder::new();
        for chunk in data.chunks(997) {
            enc.push(chunk);
        }
        let folded = enc.finish();
        for line in folded.split("\r\n") {
            assert!(line.len() <= 76, "line too long: {}", line.len());
        }
        assert_eq!(folded.replace("\r\n", ""), encode(&data));
    }

    #[test]
    fn random_hex_shape() {
        let a = random_hex(24);
        assert_eq!(a.len(), 24);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        let b = random_hex(24);
        assert_ne!(a, b);
    }

    /// Fold at 76 columns with CRLF, mirroring the MIME requirement.
    fn fold(s: &str) -> String {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && i % LINE_WIDTH == 0 {
                out.push_str("\r\n");
            }
            out.push(ch);
        }
        out
    }
}
