//! A v4 UUID, for the one thing this console has to mint: the id of a conversation it starts.
//!
//! `claude --session-id` requires a valid UUID, and this crate's dependency list is part of what it
//! promises (`Cargo.toml`), so this is done here rather than by adding a crate. Sixteen bytes from the
//! kernel's entropy pool, two of them masked to the version and variant RFC 4122 requires, and the
//! shape checked against the validator this crate already owns (`probe::uuid_token`, which is what
//! reads session ids out of transcript file names).

use std::io::Read;

/// A random v4 UUID, read from the kernel's entropy pool.
///
/// The `Err` is `/dev/urandom` being unreadable at all, which on Linux is not a state this console can
/// work around: the caller shows it as a refusal, because a turn that cannot be given an id is a turn
/// that cannot be resumed afterwards.
pub fn v4() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("/dev/urandom: {error}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_id_survives_the_validator_the_console_already_owns() {
        // `/dev/urandom` is readable on every platform this crate runs on, so this unwrap is a real
        // assertion about the mint rather than a mock: the `Err` path cannot be reached here and is
        // not worth a test that fakes a filesystem to prove nothing.
        let minted = v4().expect("the kernel's entropy pool is readable");
        // The same check the probe layer applies to a session transcript's file name, so a mint this
        // passes is a mint `--session-id` accepts -- not a shape this module invents for itself.
        assert_eq!(crate::probe::uuid_token(&minted), Some(minted.clone()));
        assert_eq!(minted.len(), 36);
        assert_eq!(minted.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn two_mints_differ() {
        // A conversation's id is the key its turns are resumed by, so two mints that agreed would be
        // two conversations the CLI refuses to tell apart -- and `--session-id` refuses an id whose
        // session record already exists, so a repeat would be a turn that never starts.
        let first = v4().expect("the kernel's entropy pool is readable");
        let second = v4().expect("the kernel's entropy pool is readable");
        assert_ne!(first, second);
    }

    #[test]
    fn the_minted_id_carries_the_version_and_variant_the_rfc_requires() {
        let minted = v4().expect("the kernel's entropy pool is readable");
        let bytes = minted.as_bytes();
        assert_eq!(bytes[14], b'4', "the version nibble: {minted}");
        assert!(
            matches!(bytes[19], b'8' | b'9' | b'a' | b'b'),
            "the variant nibble: {minted}"
        );
        assert!(
            minted.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "lowercase hex and hyphens only: {minted}"
        );
    }
}
