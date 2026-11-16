//! SHA-256 digests backed by RustCrypto.
use sha2::{Digest, Sha256};

/// Lower-case hexadecimal SHA-256 of the concatenation of `parts`.
pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}

/// Streaming SHA-256 and the first four bytes of a reader holding at most `limit` bytes.
/// `None` when it holds more (or fewer than four): never a digest of a partial read.
pub(crate) fn sha256_reader(
    reader: &mut impl std::io::Read,
    limit: u64,
) -> std::io::Result<Option<(String, [u8; 4])>> {
    let mut digest = Sha256::new();
    let mut head = Vec::with_capacity(4);
    let mut total = 0u64;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        total += n as u64;
        if total > limit {
            return Ok(None);
        }
        head.extend(chunk[..n].iter().take(4 - head.len()));
        digest.update(&chunk[..n]);
    }
    Ok(<[u8; 4]>::try_from(head)
        .ok()
        .map(|head| (format!("{:x}", digest.finalize()), head)))
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn matches_fips_180_4_vectors() {
        assert_eq!(
            sha256_hex(&[]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(&[b"abc"]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(&[b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"]),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million_a = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&[&million_a]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn part_boundaries_do_not_change_the_digest() {
        let message: Vec<u8> = (0..=255u8).cycle().take(200).collect();
        let whole = sha256_hex(&[&message]);
        for split in [0, 1, 55, 56, 63, 64, 65, 119, 120, 199, 200] {
            let (left, right) = message.split_at(split);
            assert_eq!(sha256_hex(&[left, right]), whole, "split at {split}");
        }
    }
}
