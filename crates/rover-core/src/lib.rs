//! Domain primitives for Rover's Rust implementation.
//!
//! This crate currently ports identifiers, repository/project identity,
//! raw-byte SHA-256 digests, and UTC timestamps. It does not provide process
//! execution, CLI, or TUI behavior.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as Sha2Digest, Sha256};
use std::fmt;
use std::str::FromStr;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

mod repository;
mod task;

pub use repository::{control_audience, RepositoryIdentity};
pub use task::{validate_task_graph, TaskBrief, TaskBriefError, TaskGraphError, TaskGraphNode};

/// Current serialized Rover schema marker.
pub const SCHEMA: &str = "rover/v1alpha1";
const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

/// A validated Rover identifier.
///
/// The accepted format matches the current Go model: 1–128 ASCII bytes, an
/// initial ASCII letter or digit, followed by ASCII letters, digits, `_`, `.`
/// or `-`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(String);

impl Id {
    /// Validate and construct an identifier.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidId`] when the input is empty, longer than 128 bytes,
    /// starts with a non-alphanumeric ASCII byte, or contains a disallowed
    /// character.
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidId> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid_start = bytes.first().is_some_and(u8::is_ascii_alphanumeric);
        let valid_tail = bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));

        if !valid_start || bytes.len() > 128 || !valid_tail {
            return Err(InvalidId);
        }
        Ok(Self(value))
    }

    /// Generate a validated identifier with 128 bits of operating-system
    /// random entropy, rendered as 32 lowercase hexadecimal characters.
    ///
    /// Prefixes must themselves be valid Rover ID fragments, start with an
    /// ASCII letter or digit, and leave room for the underscore and entropy.
    ///
    /// # Errors
    ///
    /// Returns [`IdGenerationError::InvalidPrefix`] for a prefix that cannot
    /// form a valid ID, or [`IdGenerationError::Random`] if the operating
    /// system's cryptographic random source fails.
    pub fn generate(prefix: &str) -> Result<Self, IdGenerationError> {
        let prefix_id = Self::parse(prefix).map_err(IdGenerationError::InvalidPrefix)?;
        if prefix_id.as_str().len() > 95 {
            return Err(IdGenerationError::InvalidPrefix(InvalidId));
        }

        let value = try_id(prefix_id.as_str()).map_err(IdGenerationError::Random)?;
        Self::parse(value).map_err(IdGenerationError::InvalidPrefix)
    }

    /// Return the validated identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Id {
    type Err = InvalidId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Error returned when an identifier does not match Rover's compatibility rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "identifier must be 1-128 ASCII characters, start with a letter or digit, and contain only letters, digits, '_', '.' or '-'",
        )
    }
}

impl std::error::Error for InvalidId {}

/// Failure while generating an identifier.
#[derive(Debug)]
pub enum IdGenerationError {
    /// The prefix cannot produce a valid identifier within Rover's length cap.
    InvalidPrefix(InvalidId),
    /// The operating system's random source failed.
    Random(getrandom::Error),
}

impl fmt::Display for IdGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPrefix(error) => write!(formatter, "invalid ID prefix: {error}"),
            Self::Random(error) => {
                write!(formatter, "operating-system random source failed: {error}")
            }
        }
    }
}

impl std::error::Error for IdGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPrefix(error) => Some(error),
            Self::Random(error) => Some(error),
        }
    }
}

/// Generate an ID string using the legacy prefix-and-random-bytes format.
///
/// This compatibility function preserves the Go model's behavior for any
/// prefix, including prefixes that do not themselves form a valid [`Id`].
/// Use [`Id::generate`] when the result must be validated as a typed ID.
///
/// # Errors
///
/// Returns the operating-system random-source error without a predictable
/// fallback.
pub fn try_id(prefix: &str) -> Result<String, getrandom::Error> {
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy)?;

    let mut value = String::with_capacity(prefix.len() + 33);
    value.push_str(prefix);
    value.push('_');
    for byte in entropy {
        value.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        value.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    Ok(value)
}

/// A SHA-256 digest with Rover's lowercase 64-character wire representation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// Hash raw bytes with SHA-256.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let hash = Sha256::digest(bytes);
        let mut output = [0; 32];
        output.copy_from_slice(&hash);
        Self(output)
    }

    /// Parse Rover's canonical lowercase hexadecimal digest representation.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidDigest`] unless the value has exactly 64 lowercase
    /// hexadecimal characters.
    pub fn parse_hex(value: &str) -> Result<Self, InvalidDigest> {
        let bytes = value.as_bytes();
        if bytes.len() != 64
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(InvalidDigest);
        }

        let mut digest = [0; 32];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            digest[index] = (decode_hex(pair[0]) << 4) | decode_hex(pair[1]);
        }
        Ok(Self(digest))
    }

    /// Return the digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Return the canonical lowercase hexadecimal representation.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
            output.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

const fn decode_hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for Sha256Digest {
    type Err = InvalidDigest;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_hex(value)
    }
}

/// Error returned when a SHA-256 digest is not in canonical Rover form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDigest;

impl fmt::Display for InvalidDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SHA-256 digest must be 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for InvalidDigest {}

/// Return the lowercase hexadecimal SHA-256 digest of raw bytes.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    Sha256Digest::of(bytes).to_string()
}

/// A typed UTC instant formatted as `RFC3339Nano` when displayed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Create a timestamp from an instant, canonicalized to UTC.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTimestamp`] if converting the instant to UTC exceeds
    /// the supported date range.
    pub fn from_instant(instant: OffsetDateTime) -> Result<Self, InvalidTimestamp> {
        instant
            .checked_to_offset(time::UtcOffset::UTC)
            .map(Self)
            .ok_or(InvalidTimestamp)
    }

    /// Capture the current system time in UTC.
    #[must_use]
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Return the timestamp as RFC3339Nano-compatible UTC text.
    ///
    /// # Errors
    ///
    /// Returns a formatting error if the instant cannot be represented in
    /// RFC3339.
    pub fn to_rfc3339(self) -> Result<String, time::error::Format> {
        self.0.format(&Rfc3339)
    }

    /// Return the underlying canonical UTC instant.
    #[must_use]
    pub const fn as_instant(&self) -> OffsetDateTime {
        self.0
    }
}

/// Error returned when an instant cannot be represented as a UTC timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTimestamp;

impl fmt::Display for InvalidTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("timestamp cannot be represented in UTC")
    }
}

impl std::error::Error for InvalidTimestamp {}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_rfc3339() {
            Ok(value) => formatter.write_str(&value),
            Err(_) => Err(fmt::Error),
        }
    }
}

/// Return the current time as a typed UTC timestamp.
#[must_use]
pub fn now() -> Timestamp {
    Timestamp::now()
}

#[cfg(test)]
mod tests {
    use super::{
        digest, now, try_id, Id, IdGenerationError, InvalidDigest, InvalidId, Sha256Digest,
        Timestamp, SCHEMA,
    };
    use std::str::FromStr;
    use time::OffsetDateTime;

    #[test]
    fn accepts_compatibility_examples_and_preserves_spelling() {
        for input in ["a", "0", "Issue_123", "task.name-2", &"x".repeat(128)] {
            let parsed = Id::parse(input).expect("valid compatibility ID");
            assert_eq!(parsed.as_str(), input);
            assert_eq!(parsed.to_string(), input);
        }
    }

    #[test]
    fn rejects_empty_non_ascii_and_invalid_characters() {
        for input in [
            "",
            "_leading",
            ".leading",
            "-leading",
            "has space",
            "é",
            "a/b",
            "a\\b",
            "a\nb",
        ] {
            assert_eq!(Id::parse(input), Err(InvalidId), "input: {input:?}");
        }
    }

    #[test]
    fn enforces_the_128_byte_limit() {
        assert!(Id::parse("x".repeat(128)).is_ok());
        assert_eq!(Id::parse("x".repeat(129)), Err(InvalidId));
    }

    #[test]
    fn from_str_uses_the_same_validation() {
        assert_eq!(Id::from_str("agent-1").unwrap().as_str(), "agent-1");
        assert_eq!(Id::from_str("agent 1"), Err(InvalidId));
    }

    #[test]
    fn exposes_the_current_schema_marker() {
        assert_eq!(SCHEMA, "rover/v1alpha1");
    }

    #[test]
    fn generates_valid_ids_with_lowercase_128_bit_entropy() {
        let id = Id::generate("task").expect("operating system random source");
        let (prefix, entropy) = id.as_str().split_once('_').expect("separator");
        assert_eq!(prefix, "task");
        assert_eq!(entropy.len(), 32);
        assert!(entropy
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(Id::parse(id.as_str()).is_ok());
    }

    #[test]
    fn legacy_generator_preserves_arbitrary_prefixes() {
        let generated = try_id("legacy prefix").expect("operating system random source");
        let (prefix, entropy) = generated.rsplit_once('_').expect("separator");
        assert_eq!(prefix, "legacy prefix");
        assert_eq!(entropy.len(), 32);
        assert!(entropy
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn generation_rejects_prefixes_without_valid_id_capacity() {
        for prefix in ["", "_bad", "has space", &"x".repeat(96)] {
            assert!(matches!(
                Id::generate(prefix),
                Err(IdGenerationError::InvalidPrefix(_))
            ));
        }
    }

    #[test]
    fn sha256_matches_known_answer_vectors() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let typed = Sha256Digest::of(b"abc");
        assert_eq!(typed.to_hex(), digest(b"abc"));
        assert_eq!(Sha256Digest::parse_hex(&typed.to_string()), Ok(typed));
        assert_eq!(typed.as_bytes().len(), 32);
    }

    #[test]
    fn digest_validation_rejects_noncanonical_values() {
        let uppercase = "A".repeat(64);
        let too_short = "a".repeat(63);
        let non_hex = "g".repeat(64);
        for invalid in ["", uppercase.as_str(), too_short.as_str(), non_hex.as_str()] {
            assert_eq!(Sha256Digest::parse_hex(invalid), Err(InvalidDigest));
        }
    }

    #[test]
    fn utc_format_trims_fractional_zeroes_like_rfc3339nano() {
        let instant = OffsetDateTime::from_unix_timestamp(0).unwrap()
            + time::Duration::nanoseconds(123_400_000);
        let typed = Timestamp::from_instant(instant).unwrap();
        assert_eq!(typed.to_rfc3339().unwrap(), "1970-01-01T00:00:00.1234Z");
        assert_eq!(typed.to_string(), "1970-01-01T00:00:00.1234Z");
        assert_eq!(
            Timestamp::from_instant(OffsetDateTime::from_unix_timestamp(0).unwrap())
                .unwrap()
                .to_rfc3339()
                .unwrap(),
            "1970-01-01T00:00:00Z"
        );

        let maximum_fraction = OffsetDateTime::from_unix_timestamp(0).unwrap()
            + time::Duration::nanoseconds(999_999_999);
        assert_eq!(
            Timestamp::from_instant(maximum_fraction)
                .unwrap()
                .to_rfc3339()
                .unwrap(),
            "1970-01-01T00:00:00.999999999Z"
        );
    }

    #[test]
    fn timestamp_canonicalizes_offsets_to_utc() {
        let instant = OffsetDateTime::from_unix_timestamp(0)
            .unwrap()
            .to_offset(time::UtcOffset::from_hms(-5, 0, 0).unwrap());
        let timestamp = Timestamp::from_instant(instant).unwrap();
        assert_eq!(timestamp.as_instant().offset(), time::UtcOffset::UTC);
        assert_eq!(timestamp.to_rfc3339().unwrap(), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn current_timestamp_has_utc_rfc3339_shape() {
        let value = now()
            .to_rfc3339()
            .expect("system timestamp in RFC3339 range");
        let timestamp = value.strip_suffix('Z').expect("UTC suffix");
        let (date, clock) = timestamp.split_once('T').expect("date/time separator");
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        assert!(date
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()));
        let (clock, fraction) = clock
            .split_once('.')
            .map_or((clock, None), |(clock, fraction)| (clock, Some(fraction)));
        assert_eq!(clock.len(), 8);
        assert_eq!(&clock[2..3], ":");
        assert_eq!(&clock[5..6], ":");
        assert!(clock
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 2 | 5) || byte.is_ascii_digit()));
        if let Some(fraction) = fraction {
            assert!((1..=9).contains(&fraction.len()));
            assert!(fraction.bytes().all(|byte| byte.is_ascii_digit()));
            assert!(!fraction.ends_with('0'));
        }
    }
}
