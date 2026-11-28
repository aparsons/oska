// SPDX-License-Identifier: Apache-2.0
//! SHA-256 (FIPS 180-4), for the build cache's keys and its entries'
//! integrity check (`cache.rs`). Written here because the workspace
//! takes no crates from outside the repo, and without `unsafe` (ADR 0009).
//! Oska's own `std.sha2` (#231) is for Oska programs, not for `oskac`.

/// The round constants: the first 32 bits of the fractional parts of the
/// cube roots of the first 64 primes.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The initial hash value: the first 32 bits of the fractional parts of
/// the square roots of the first 8 primes.
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// A SHA-256 digest.
pub type Digest = [u8; 32];

/// A SHA-256 computation, fed with [`Sha256::update`].
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    /// The bytes of a block not yet full.
    block: [u8; 64],
    filled: usize,
    /// The message length so far, in bytes.
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Sha256 {
        Sha256::new()
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256 {
            state: H0,
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    /// Hash `data` after what came before.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.filled > 0 {
            let take = data.len().min(64 - self.filled);
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            self.compress(&block);
            self.filled = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            self.compress(block);
        }
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    /// The digest of everything hashed.
    pub fn finish(mut self) -> Digest {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut digest = [0u8; 32];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            out.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// The compression function, on one 64-byte block.
    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

/// The SHA-256 digest of `data`.
pub fn digest(data: &[u8]) -> Digest {
    let mut sha = Sha256::new();
    sha.update(data);
    sha.finish()
}

/// `digest` as 64 lowercase hex digits.
pub fn hex(digest: &Digest) -> String {
    use std::fmt::Write;
    digest
        .iter()
        .fold(String::with_capacity(64), |mut text, b| {
            let _ = write!(text, "{b:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The examples of FIPS 180-4 (NIST's "SHA-256" example values) and
    /// the short and long messages of the SHA Validation System's
    /// `SHA256ShortMsg.rsp` and `SHA256LongMsg.rsp`.
    #[test]
    fn nist_test_vectors() {
        let vectors: &[(&[u8], &str)] = &[
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
            // SHA256ShortMsg.rsp, Len = 8 and Len = 512.
            (
                &[0xd3],
                "28969cdfa74a12c82f3bad960b0b000aca2ac329deea5c2328ebc6f2ba9802c1",
            ),
            (
                &[
                    0x5a, 0x86, 0xb7, 0x37, 0xea, 0xea, 0x8e, 0xe9, 0x76, 0xa0, 0xa2, 0x4d, 0xa6,
                    0x3e, 0x7e, 0xd7, 0xee, 0xfa, 0xd1, 0x8a, 0x10, 0x1c, 0x12, 0x11, 0xe2, 0xb3,
                    0x65, 0x0c, 0x51, 0x87, 0xc2, 0xa8, 0xa6, 0x50, 0x54, 0x72, 0x08, 0x25, 0x1f,
                    0x6d, 0x42, 0x37, 0xe6, 0x61, 0xc7, 0xbf, 0x4c, 0x77, 0xf3, 0x35, 0x39, 0x03,
                    0x94, 0xc3, 0x7f, 0xa1, 0xa9, 0xf9, 0xbe, 0x83, 0x6a, 0xc2, 0x85, 0x09,
                ],
                "42e61e174fbb3897d6dd6cef3dd2802fe67b331953b06114a65c772859dfc1aa",
            ),
        ];
        for (message, expected) in vectors {
            assert_eq!(hex(&digest(message)), *expected, "{message:02x?}");
        }
    }

    /// FIPS 180-4's long example: one million `a`s.
    #[test]
    fn a_million_as() {
        let mut sha = Sha256::new();
        for _ in 0..1000 {
            sha.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex(&sha.finish()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let message: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();
        let whole = digest(&message);
        for split in [1, 55, 56, 63, 64, 65, 128, 299] {
            let mut sha = Sha256::new();
            for chunk in message.chunks(split) {
                sha.update(chunk);
            }
            assert_eq!(sha.finish(), whole, "chunks of {split}");
        }
    }
}
