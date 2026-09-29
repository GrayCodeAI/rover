//! Locally issued bearer grants bound to an exact project and state root.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use getrandom::fill;
use rover_core::{control_audience, digest, now, Id, RepositoryIdentity, SCHEMA};
use rover_store::{Store, StoreError};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const TOKEN_PREFIX: &str = "rvr_";
const MIN_TTL: Duration = Duration::from_secs(60);
const MAX_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_TOOLS: usize = 32;
const MAX_NOTE_BYTES: usize = 4096;
const TOKEN_ENTROPY_BYTES: usize = 32;
const TOKEN_LEN: usize = 47;
const GRANT_KIND: &str = "grant";

const ALLOWED_TOOLS: &[&str] = &[
    "rover_agent_capabilities",
    "rover_inspect",
    "rover_status",
    "rover_report",
    "rover_diff",
    "rover_context_search",
    "rover_verify",
    "rover_task_run",
    "rover_task_cancel",
];

/// A persisted grant. The raw bearer token is never stored in the record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Grant {
    /// Rover schema marker.
    pub schema: String,
    /// SHA-256 of the raw bearer token.
    pub id: String,
    /// Audience binds the state root and repository.
    pub audience: String,
    /// SHA-256 project identity for the canonical repository path.
    pub project: String,
    /// Tool names allowed by this grant.
    pub tools: Vec<String>,
    /// Human supplied reason for issuance.
    pub note: String,
    /// RFC3339 creation time.
    pub created_at: String,
    /// RFC3339 expiry time.
    pub expires_at: String,
    /// Whether the grant was revoked.
    pub revoked: bool,
}

impl Grant {
    /// Check whether this grant names a tool.
    #[must_use]
    pub fn allows(&self, tool: &str) -> bool {
        self.tools.iter().any(|allowed| allowed == tool)
    }
}

/// Failure while issuing, authenticating, or revoking a local grant.
#[derive(Debug)]
pub enum GrantError {
    /// Request parameters violate the local grant contract.
    InvalidRequest,
    /// Repository path could not be made absolute.
    RepositoryPath(std::io::Error),
    /// The operating system's cryptographic random source failed.
    Random(getrandom::Error),
    /// The grant store failed.
    Store(StoreError),
    /// The grant could not be represented with a valid expiration timestamp.
    Timestamp,
    /// Authentication failed. Deliberately does not disclose which check failed.
    Denied,
}

impl fmt::Display for GrantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => formatter.write_str("invalid grant request"),
            Self::RepositoryPath(error) => write!(formatter, "repository identity failed: {error}"),
            Self::Random(error) => write!(formatter, "grant token generation failed: {error}"),
            Self::Store(error) => write!(formatter, "grant store failed: {error}"),
            Self::Timestamp => {
                formatter.write_str("grant timestamp is outside the supported range")
            }
            Self::Denied => formatter.write_str("unauthorized or expired grant"),
        }
    }
}

impl std::error::Error for GrantError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RepositoryPath(error) => Some(error),
            Self::Random(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::InvalidRequest | Self::Timestamp | Self::Denied => None,
        }
    }
}

/// Grant operations over one Rover record store.
pub struct GrantService<'store> {
    store: &'store Store,
}

impl<'store> GrantService<'store> {
    /// Bind grant operations to a Rover state store.
    #[must_use]
    pub const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// Issue and persist a project-scoped bearer grant.
    ///
    /// # Errors
    ///
    /// Returns [`GrantError::InvalidRequest`] for an empty/duplicate/unknown
    /// tool list, invalid note, or TTL outside 1 minute through 30 days.
    /// Returns a store, path, entropy, or timestamp error if issuance fails.
    pub fn issue(
        &self,
        state_root: &Path,
        repository: &str,
        tools: &[&str],
        note: &str,
        ttl: Duration,
    ) -> Result<(Grant, String), GrantError> {
        if tools.is_empty()
            || tools.len() > MAX_TOOLS
            || !(MIN_TTL..=MAX_TTL).contains(&ttl)
            || note.trim().is_empty()
            || note.len() > MAX_NOTE_BYTES
        {
            return Err(GrantError::InvalidRequest);
        }
        let mut seen = Vec::with_capacity(tools.len());
        for tool in tools {
            Id::parse(*tool).map_err(|_| GrantError::InvalidRequest)?;
            if !ALLOWED_TOOLS.contains(tool) || seen.contains(tool) {
                return Err(GrantError::InvalidRequest);
            }
            seen.push(tool);
        }

        let identity =
            RepositoryIdentity::resolve(repository).map_err(GrantError::RepositoryPath)?;
        let mut entropy = [0_u8; TOKEN_ENTROPY_BYTES];
        fill(&mut entropy).map_err(GrantError::Random)?;
        let token = format!("{TOKEN_PREFIX}{}", base64url_no_pad(&entropy));
        let token_digest = digest(token.as_bytes());
        let created_at = now().to_rfc3339().map_err(|_| GrantError::Timestamp)?;
        let expiry_duration = time::Duration::try_from(ttl).map_err(|_| GrantError::Timestamp)?;
        let expires_at = OffsetDateTime::now_utc()
            .checked_add(expiry_duration)
            .ok_or(GrantError::Timestamp)?
            .format(&Rfc3339)
            .map_err(|_| GrantError::Timestamp)?;
        let grant = Grant {
            schema: SCHEMA.to_owned(),
            id: token_digest.clone(),
            audience: control_audience(state_root, &identity),
            project: identity.project_id().to_hex(),
            tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
            note: note.to_owned(),
            created_at,
            expires_at,
            revoked: false,
        };
        self.store
            .put(GRANT_KIND, &grant.id, &grant, "grant.created")
            .map_err(GrantError::Store)?;
        Ok((grant, token))
    }

    /// Authenticate a bearer token against the exact state root and project.
    ///
    /// All malformed, missing, expired, revoked, and cross-scope tokens return
    /// the same [`GrantError::Denied`] value.
    ///
    /// # Errors
    ///
    /// Returns [`GrantError::Denied`] for any invalid, missing, expired,
    /// revoked, or differently scoped token.
    pub fn authenticate(
        &self,
        state_root: &Path,
        repository: &str,
        token: &str,
    ) -> Result<Grant, GrantError> {
        if token.len() != TOKEN_LEN || !token.starts_with(TOKEN_PREFIX) {
            return Err(GrantError::Denied);
        }
        let id = digest(token.as_bytes());
        let grant: Grant = self
            .store
            .get_typed(GRANT_KIND, &id)
            .map_err(|_| GrantError::Denied)?;
        let identity = RepositoryIdentity::resolve(repository).map_err(|_| GrantError::Denied)?;
        let expiration =
            OffsetDateTime::parse(&grant.expires_at, &Rfc3339).map_err(|_| GrantError::Denied)?;
        if grant.revoked
            || OffsetDateTime::now_utc() >= expiration
            || grant.audience != control_audience(state_root, &identity)
            || grant.project != identity.project_id().to_hex()
        {
            return Err(GrantError::Denied);
        }
        Ok(grant)
    }

    /// Revoke a grant and append its event in one store transaction.
    ///
    /// # Errors
    ///
    /// Returns [`GrantError::InvalidRequest`] for an invalid record ID or
    /// [`GrantError::Store`] if the grant is missing or the mutation fails.
    pub fn revoke(&self, id: &str) -> Result<(), GrantError> {
        Id::parse(id).map_err(|_| GrantError::InvalidRequest)?;
        self.store
            .mutate(GRANT_KIND, id, "grant.revoked", |value| {
                let mut grant: Grant =
                    serde_json::from_value(value.clone()).map_err(StoreError::Json)?;
                grant.revoked = true;
                serde_json::to_value(grant).map_err(StoreError::Json)
            })
            .map_err(GrantError::Store)
    }
}

fn base64url_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() >= 2 {
            encoded.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        }
        if chunk.len() == 3 {
            encoded.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use rover_store::Store;
    use rusqlite::Connection;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};

    use super::{base64url_no_pad, Grant, GrantError, GrantService, MAX_TTL};

    fn memory_store() -> Store {
        Store::from_connection(Connection::open_in_memory().expect("open memory database"))
            .expect("migrate memory database")
    }

    #[test]
    fn base64url_encoder_matches_rfc4648_unpadded_vectors() {
        assert_eq!(base64url_no_pad(b"f"), "Zg");
        assert_eq!(base64url_no_pad(b"fo"), "Zm8");
        assert_eq!(base64url_no_pad(b"foo"), "Zm9v");
        assert_eq!(base64url_no_pad(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn grant_authentication_binds_state_and_project_then_revoke_denies() {
        let store = memory_store();
        let service = GrantService::new(&store);
        let (grant, token) = service
            .issue(
                Path::new("/state/one"),
                "/project/a",
                &["rover_status", "rover_diff"],
                "owned client",
                Duration::from_secs(60),
            )
            .expect("issue grant");
        assert_eq!(token.len(), 47);
        assert!(token.starts_with("rvr_"));
        assert_eq!(grant.id, rover_core::digest(token.as_bytes()));
        let authenticated = service
            .authenticate(Path::new("/state/one"), "/project/a", &token)
            .expect("authenticate grant");
        assert!(authenticated.allows("rover_status"));
        assert!(!authenticated.allows("rover_verify"));
        assert!(matches!(
            service.authenticate(Path::new("/state/one"), "/project/b", &token),
            Err(GrantError::Denied)
        ));
        assert!(matches!(
            service.authenticate(Path::new("/state/two"), "/project/a", &token),
            Err(GrantError::Denied)
        ));
        service.revoke(&grant.id).expect("revoke grant");
        assert!(matches!(
            service.authenticate(Path::new("/state/one"), "/project/a", &token),
            Err(GrantError::Denied)
        ));
    }

    #[test]
    fn grant_request_validation_matches_tool_note_and_ttl_bounds() {
        let store = memory_store();
        let service = GrantService::new(&store);
        assert!(matches!(
            service.issue(
                Path::new("/state"),
                "/repo",
                &[],
                "reason",
                Duration::from_secs(60)
            ),
            Err(GrantError::InvalidRequest)
        ));
        for (tools, note, ttl) in [
            (vec!["not_a_tool"], "reason", Duration::from_secs(60)),
            (
                vec!["rover_status", "rover_status"],
                "reason",
                Duration::from_secs(60),
            ),
            (vec!["rover_status"], "  ", Duration::from_secs(60)),
            (vec!["rover_status"], "reason", Duration::from_secs(59)),
            (
                vec!["rover_status"],
                "reason",
                MAX_TTL + Duration::from_nanos(1),
            ),
        ] {
            assert!(matches!(
                service.issue(Path::new("/state"), "/repo", &tools, note, ttl),
                Err(GrantError::InvalidRequest)
            ));
        }
        assert!(service
            .issue(
                Path::new("/state"),
                "/repo",
                &["rover_status"],
                &"n".repeat(4096),
                MAX_TTL
            )
            .is_ok());
        assert!(matches!(
            service.issue(
                Path::new("/state"),
                "/repo",
                &["rover_status"],
                &"n".repeat(4097),
                Duration::from_secs(60)
            ),
            Err(GrantError::InvalidRequest)
        ));
    }

    #[test]
    fn malformed_expiry_and_token_are_denied_without_leaking_reason() {
        let store = memory_store();
        let service = GrantService::new(&store);
        let (mut grant, token) = service
            .issue(
                Path::new("/state"),
                "/repo",
                &["rover_status"],
                "reason",
                Duration::from_secs(60),
            )
            .expect("issue grant");
        assert!(matches!(
            service.authenticate(Path::new("/state"), "/repo", "wrong"),
            Err(GrantError::Denied)
        ));
        grant.expires_at = (OffsetDateTime::now_utc() - time::Duration::seconds(1))
            .format(&Rfc3339)
            .expect("format expired timestamp");
        store
            .put("grant", &grant.id, &grant, "")
            .expect("set expired timestamp");
        assert!(matches!(
            service.authenticate(Path::new("/state"), "/repo", &token),
            Err(GrantError::Denied)
        ));
    }

    #[test]
    fn grant_record_round_trips_with_go_field_names() {
        let store = memory_store();
        let service = GrantService::new(&store);
        let (grant, _) = service
            .issue(
                Path::new("/state"),
                "/repo",
                &["rover_status"],
                "reason",
                Duration::from_secs(60),
            )
            .expect("issue grant");
        let saved: Grant = store
            .get_typed("grant", &grant.id)
            .expect("read grant record");
        assert_eq!(saved, grant);
        let json = serde_json::to_value(saved).expect("serialize grant");
        assert_eq!(json["created_at"], grant.created_at);
        assert_eq!(json["expires_at"], grant.expires_at);
        assert_eq!(json["revoked"], false);
    }
}
