/// Byte-level statistics over a raw byte slice.
///
/// All fields are in [0.0, 1.0] except `entropy` which is in [0.0, 8.0]
/// (bits per byte). Computed in a single pass with a 256-entry stack histogram
/// — no heap allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ByteStats {
    /// Shannon entropy H = -Σ p(b) log₂ p(b) in bits (0 = uniform, 8 = max).
    pub entropy: f64,
    /// Fraction of bytes that are printable ASCII (0x20–0x7E) or common
    /// whitespace (0x09 tab, 0x0A LF, 0x0D CR).
    pub printable_ratio: f64,
    /// Fraction of zero bytes (0x00).
    pub null_ratio: f64,
    /// Fraction of bytes in the hexadecimal alphabet (0-9, a-f, A-F).
    pub hex_density: f64,
    /// Fraction of bytes in the base-64 alphabet (A-Z, a-z, 0-9, +, /, =).
    pub base64_density: f64,
}

impl ByteStats {
    /// Returns a `ByteStats` with all fields at 0. Used for empty slices.
    pub fn zero() -> Self {
        ByteStats { entropy: 0.0, printable_ratio: 0.0, null_ratio: 0.0, hex_density: 0.0, base64_density: 0.0 }
    }
}

/// Compute byte statistics over `bytes`. Returns `ByteStats::zero()` for an
/// empty slice.
pub fn byte_stats(bytes: &[u8]) -> ByteStats {
    if bytes.is_empty() {
        return ByteStats::zero();
    }

    let mut hist = [0u64; 256];
    let mut printable: u64 = 0;
    let mut nulls: u64 = 0;
    let mut hex: u64 = 0;
    let mut b64: u64 = 0;

    for &b in bytes {
        hist[b as usize] += 1;

        if (b >= 0x20 && b <= 0x7E) || b == 0x09 || b == 0x0A || b == 0x0D {
            printable += 1;
        }
        if b == 0x00 {
            nulls += 1;
        }
        if (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F') {
            hex += 1;
        }
        if (b >= b'A' && b <= b'Z')
            || (b >= b'a' && b <= b'z')
            || (b >= b'0' && b <= b'9')
            || b == b'+'
            || b == b'/'
            || b == b'='
        {
            b64 += 1;
        }
    }

    let n = bytes.len() as f64;
    let entropy = hist.iter().fold(0.0f64, |acc, &count| {
        if count == 0 {
            acc
        } else {
            let p = count as f64 / n;
            acc - p * p.log2()
        }
    });

    ByteStats {
        entropy,
        printable_ratio: printable as f64 / n,
        null_ratio: nulls as f64 / n,
        hex_density: hex as f64 / n,
        base64_density: b64 as f64 / n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        let s = byte_stats(&[]);
        assert_eq!(s.entropy, 0.0);
        assert_eq!(s.printable_ratio, 0.0);
    }

    #[test]
    fn uniform_bytes_max_entropy() {
        let buf: Vec<u8> = (0u8..=255).collect();
        let s = byte_stats(&buf);
        assert!((s.entropy - 8.0).abs() < 1e-10, "entropy={}", s.entropy);
    }

    #[test]
    fn single_byte_zero_entropy() {
        let buf = vec![0x41u8; 1024];
        let s = byte_stats(&buf);
        assert_eq!(s.entropy, 0.0);
        assert_eq!(s.printable_ratio, 1.0); // 0x41 = 'A', printable
    }

    #[test]
    fn null_bytes() {
        let buf = vec![0x00u8; 100];
        let s = byte_stats(&buf);
        assert_eq!(s.null_ratio, 1.0);
        assert_eq!(s.entropy, 0.0);
    }

    #[test]
    fn printable_ascii_source() {
        // Normal source code is mostly printable ASCII
        let src = b"fn main() { println!(\"hello\"); }\n";
        let s = byte_stats(src);
        assert!(s.printable_ratio > 0.9, "printable_ratio={}", s.printable_ratio);
        assert!(s.null_ratio == 0.0);
    }

    #[test]
    fn hex_string_density() {
        let hex_str = b"deadbeef0123456789abcdef";
        let s = byte_stats(hex_str);
        assert_eq!(s.hex_density, 1.0);
    }
}
