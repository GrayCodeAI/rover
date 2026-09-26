//! Rover's `SQLite` migrations and transactional local record APIs.
//!
//! This crate accepts caller-supplied connections. Filesystem state-root
//! admission, content-addressed blobs, and backup remain separate contracts.

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

use rover_core::{Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub mod budget;
#[cfg(unix)]
pub mod files;
#[cfg(unix)]
pub mod legacy;
pub mod resources;

pub const SCHEMA_VERSION: i64 = 1;
const MIGRATION_NAME: &str = "0001_rover_v1_records_events_request_keys";
const MIGRATION_SQL: &str = "CREATE TABLE records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id));\
CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL);\
CREATE TABLE request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL);";
const LEDGER_SQL: &str = "CREATE TABLE schema_migrations (\
    version INTEGER PRIMARY KEY CHECK(version > 0),\
    name TEXT NOT NULL UNIQUE,\
    checksum TEXT NOT NULL CHECK(length(checksum) = 64),\
    applied_at TEXT NOT NULL\
);";

const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORD_DEPTH: usize = 10_000;
const JSON_STACK_SEGMENT_BYTES: usize = 32 * 1024 * 1024;

/// A JSON record with iterative destruction, including records at Rover's
/// maximum nesting depth. It intentionally does not implement `Clone` or
/// recursive `Debug` formatting.
pub struct RecordValue(Value);

impl RecordValue {
    /// Wrap a JSON value so the store can safely handle its destruction even
    /// when it is nested at Rover's 10,000-level limit.
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        Self(value)
    }

    /// Borrow the underlying JSON value.
    #[must_use]
    pub const fn as_value(&self) -> &Value {
        &self.0
    }

    /// Mutably borrow the wrapped JSON value.
    pub fn as_value_mut(&mut self) -> &mut Value {
        &mut self.0
    }

    /// Read a string nested under a sequence of object keys without exposing
    /// the JSON implementation type to callers.
    #[must_use]
    pub fn string_at_path<'a>(&'a self, path: &[&str]) -> Option<&'a str> {
        self.value_at_path(path)?.as_str()
    }

    /// Read a signed integer nested under object keys.
    #[must_use]
    pub fn integer_at_path(&self, path: &[&str]) -> Option<i64> {
        self.value_at_path(path)?.as_i64()
    }

    /// Read a boolean nested under object keys.
    #[must_use]
    pub fn bool_at_path(&self, path: &[&str]) -> Option<bool> {
        self.value_at_path(path)?.as_bool()
    }

    /// Return the array length at an object path.
    #[must_use]
    pub fn array_len_at_path(&self, path: &[&str]) -> Option<usize> {
        self.value_at_path(path)?.as_array().map(Vec::len)
    }

    /// Borrow array elements at an object path without copying nested JSON values.
    #[must_use]
    pub fn array_values_at_path(&self, path: &[&str]) -> Option<&[Value]> {
        self.value_at_path(path)?.as_array().map(Vec::as_slice)
    }

    /// Return whether a JSON value exists at an object path, including null.
    #[must_use]
    pub fn contains_path(&self, path: &[&str]) -> bool {
        self.value_at_path(path).is_some()
    }
}

impl RecordValue {
    fn value_at_path<'a>(&'a self, path: &[&str]) -> Option<&'a Value> {
        if path.is_empty() {
            return None;
        }
        path.iter().try_fold(&self.0, |value, key| value.get(*key))
    }
}

impl Deref for RecordValue {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for RecordValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl Drop for RecordValue {
    fn drop(&mut self) {
        let root = std::mem::replace(&mut self.0, Value::Null);
        drop_value_iteratively(root);
    }
}

fn drop_value_iteratively(root: Value) {
    let mut pending = vec![root];
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(items) => pending.extend(items),
            Value::Object(items) => pending.extend(items.into_values()),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

/// An error returned by the transactional record store.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Migration(MigrationError),
    Json(serde_json::Error),
    Encoding(std::string::FromUtf8Error),
    InvalidId(rover_core::InvalidId),
    RecordTooLarge,
    RecordTooDeep,
    InvalidBound,
    CollectionTooLarge,
    NotFound,
    IdempotencyConflict,
    MutationRejected(String),
    Timestamp(String),
    Poisoned,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite store failed: {error}"),
            Self::Migration(error) => write!(formatter, "store migration failed: {error}"),
            Self::Json(error) => write!(formatter, "record JSON failed: {error}"),
            Self::Encoding(error) => write!(
                formatter,
                "record JSON encoding produced invalid UTF-8: {error}"
            ),
            Self::InvalidId(error) => write!(formatter, "invalid record key: {error}"),
            Self::RecordTooLarge => formatter.write_str("record exceeds 16 MiB"),
            Self::RecordTooDeep => {
                formatter.write_str("record exceeds the 10000-level JSON nesting limit")
            }
            Self::InvalidBound => formatter.write_str("invalid explicit collection bound"),
            Self::CollectionTooLarge => {
                formatter.write_str("collection exceeds explicit bound; refusing partial answer")
            }
            Self::NotFound => formatter.write_str("record not found"),
            Self::IdempotencyConflict => {
                formatter.write_str("idempotency key reused with a different contract")
            }
            Self::MutationRejected(reason) => {
                write!(formatter, "record mutation rejected: {reason}")
            }
            Self::Timestamp(error) => write!(formatter, "timestamp formatting failed: {error}"),
            Self::Poisoned => formatter.write_str("SQLite store connection lock is poisoned"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::InvalidId(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
impl From<MigrationError> for StoreError {
    fn from(error: MigrationError) -> Self {
        Self::Migration(error)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<std::string::FromUtf8Error> for StoreError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Encoding(error)
    }
}
impl From<rover_core::InvalidId> for StoreError {
    fn from(error: rover_core::InvalidId) -> Self {
        Self::InvalidId(error)
    }
}

/// Transactional access to Rover records, events, and idempotency keys.
///
/// The supplied connection is configured and migrated on construction. This
/// type does not open paths or establish filesystem permissions; those policies
/// belong to the state-root opener.
pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Configure and migrate an existing `SQLite` connection.
    ///
    /// # Errors
    ///
    /// Returns an error if pragmas cannot be applied or the schema is not supported.
    pub fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Insert or replace a record and optionally append the matching event atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid IDs, oversized or over-nested values, serialization failures, or `SQLite` errors.
    pub fn put<T: Serialize>(
        &self,
        kind: &str,
        id: &str,
        value: &T,
        event: &str,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        put_in_transaction(&transaction, kind, id, value, event)?;
        transaction.commit()?;
        Ok(())
    }

    /// Read a JSON record into the dynamic JSON representation.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] when the record is missing, or an error for invalid stored JSON or `SQLite` failures.
    pub fn get(&self, kind: &str, id: &str) -> Result<RecordValue, StoreError> {
        let connection = self.lock()?;
        let payload = connection
            .query_row(
                "SELECT payload FROM records WHERE kind=?1 AND id=?2",
                (kind, id),
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        parse_payload(&payload)
    }

    /// Read and deserialize a record into a caller-selected Rust type.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] when missing, or an error when the stored JSON cannot be deserialized into `T`.
    pub fn get_typed<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T, StoreError> {
        let connection = self.lock()?;
        let payload = connection
            .query_row(
                "SELECT payload FROM records WHERE kind=?1 AND id=?2",
                (kind, id),
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        decode_payload(&payload)
    }

    /// Return stored JSON text in update order without changing number tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the `SQLite` query fails.
    pub fn list_raw(&self, kind: &str, limit: i64) -> Result<Vec<String>, StoreError> {
        let limit = if (1..=1000).contains(&limit) {
            limit
        } else {
            100
        };
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT payload FROM records WHERE kind=?1 ORDER BY updated DESC,id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((kind, limit), |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// List up to `max` raw JSON payloads, refusing partial answers.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBound`] outside 1..=100000, or [`StoreError::CollectionTooLarge`] if more records exist.
    pub fn list_all_raw(&self, kind: &str, max: i64) -> Result<Vec<String>, StoreError> {
        if !(1..=100_000).contains(&max) {
            return Err(StoreError::InvalidBound);
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT payload FROM records WHERE kind=?1 ORDER BY updated DESC,id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((kind, max + 1), |row| row.get::<_, String>(0))?;
        let payloads = rows.collect::<Result<Vec<_>, _>>()?;
        if i64::try_from(payloads.len()).unwrap_or(i64::MAX) > max {
            return Err(StoreError::CollectionTooLarge);
        }
        Ok(payloads)
    }

    /// List records ordered by update time and descending ID. Invalid limits use the Go default of 100.
    ///
    /// # Errors
    ///
    /// Returns an error if stored JSON is invalid or the database query fails.
    pub fn list(&self, kind: &str, limit: i64) -> Result<Vec<RecordValue>, StoreError> {
        let limit = if (1..=1000).contains(&limit) {
            limit
        } else {
            100
        };
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT payload FROM records WHERE kind=?1 ORDER BY updated DESC,id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((kind, limit), |row| row.get::<_, String>(0))?;
        rows.map(|row| parse_payload(&row?)).collect()
    }

    /// List at most `max` records, failing if more exist instead of returning a partial result.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBound`] for a bound outside 1..=100000, [`StoreError::CollectionTooLarge`] if more records exist, or an error for invalid stored JSON or `SQLite` failures.
    pub fn list_all(&self, kind: &str, max: i64) -> Result<Vec<RecordValue>, StoreError> {
        if !(1..=100_000).contains(&max) {
            return Err(StoreError::InvalidBound);
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT payload FROM records WHERE kind=?1 ORDER BY updated DESC,id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((kind, max + 1), |row| row.get::<_, String>(0))?;
        let payloads = rows.collect::<Result<Vec<_>, _>>()?;
        if i64::try_from(payloads.len()).unwrap_or(i64::MAX) > max {
            return Err(StoreError::CollectionTooLarge);
        }
        payloads
            .iter()
            .map(|payload| parse_payload(payload))
            .collect()
    }

    /// Read, transform, replace, and optionally append an event in one transaction.
    /// The callback executes while the database lock is held and must not perform external effects.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] for a missing record, or an error if the callback, serialization, or `SQLite` operation fails.
    pub fn mutate<F>(&self, kind: &str, id: &str, event: &str, mutate: F) -> Result<(), StoreError>
    where
        F: FnOnce(&Value) -> Result<Value, StoreError>,
    {
        self.mutate_with_event(kind, id, |old| {
            mutate(old).map(|new| (new, (!event.is_empty()).then(|| event.to_owned())))
        })
    }

    /// Read and replace a record atomically while choosing whether to append
    /// an event based on the value observed inside the transaction.
    ///
    /// This is useful when a state transition can lose a race after an
    /// external observation: the callback can return the original value and
    /// omit the event without exposing a check-then-write window.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] for a missing record, or an error from
    /// the callback, serialization, or `SQLite` operation.
    pub fn mutate_with_event<F>(&self, kind: &str, id: &str, mutate: F) -> Result<(), StoreError>
    where
        F: FnOnce(&Value) -> Result<(Value, Option<String>), StoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload = transaction
            .query_row(
                "SELECT payload FROM records WHERE kind=?1 AND id=?2",
                (kind, id),
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let old_value = parse_payload(&payload)?;
        let (new_value, event) = mutate(old_value.as_value())?;
        let new_value = RecordValue::from_value(new_value);
        put_in_transaction(
            &transaction,
            kind,
            id,
            new_value.as_value(),
            event.as_deref().unwrap_or_default(),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Mutate using the stored JSON text directly, matching Rover's Go callback contract.
    ///
    /// The callback runs inside the immediate transaction while the store lock is held.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the record is missing, or an error from the callback, serialization, or `SQLite`.
    pub fn mutate_raw<T, F>(
        &self,
        kind: &str,
        id: &str,
        event: &str,
        mutate: F,
    ) -> Result<(), StoreError>
    where
        T: Serialize,
        F: FnOnce(&str) -> Result<T, StoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload = transaction
            .query_row(
                "SELECT payload FROM records WHERE kind=?1 AND id=?2",
                (kind, id),
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let value = mutate(&payload)?;
        put_in_transaction(&transaction, kind, id, &value, event)?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomically create a record, its `{kind}.created` event, and an optional idempotency key.
    /// A matching key returns its original reference with `exists=true` and appends no event.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::IdempotencyConflict`] when a key is reused with another digest, or an error for invalid IDs, oversized values, serialization, or `SQLite` failures.
    pub fn create_once<T: Serialize>(
        &self,
        kind: &str,
        id: &str,
        key: &str,
        digest: &str,
        value: &T,
    ) -> Result<(String, bool), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !key.is_empty() {
            let prior = transaction
                .query_row(
                    "SELECT digest,ref FROM request_keys WHERE key=?1",
                    [key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((prior_digest, prior_ref)) = prior {
                if prior_digest != digest {
                    return Err(StoreError::IdempotencyConflict);
                }
                transaction.commit()?;
                return Ok((prior_ref, true));
            }
        }
        put_in_transaction(&transaction, kind, id, value, &format!("{kind}.created"))?;
        if !key.is_empty() {
            transaction.execute(
                "INSERT INTO request_keys(key,digest,ref) VALUES(?1,?2,?3)",
                (key, digest, id),
            )?;
        }
        transaction.commit()?;
        Ok((id.to_owned(), false))
    }

    /// Return up to 1000 event summaries for a record ID, in append order.
    ///
    /// # Errors
    ///
    /// Returns an error if the `SQLite` query fails.
    pub fn events(&self, id: &str) -> Result<Vec<Value>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT seq,kind,ref,at FROM events WHERE ref=?1 ORDER BY seq LIMIT 1000")?;
        let rows = statement.query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (sequence, event, reference, at) = row?;
            Ok(serde_json::json!({"sequence": sequence.to_string(), "event": event, "ref": reference, "at": at}))
        }).collect()
    }

    /// Delete an existing record and optionally append a `{deleted:true}` event atomically.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no record exists, or an error if the event cannot be serialized or the `SQLite` transaction fails.
    pub fn delete(&self, kind: &str, id: &str, event: &str) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted =
            transaction.execute("DELETE FROM records WHERE kind=?1 AND id=?2", (kind, id))?;
        if deleted == 0 {
            return Err(StoreError::NotFound);
        }
        if !event.is_empty() {
            insert_event(
                &transaction,
                event,
                id,
                &serde_json::json!({"deleted": true}),
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }
}

fn put_in_transaction(
    transaction: &Transaction<'_>,
    kind: &str,
    id: &str,
    value: &impl Serialize,
    event: &str,
) -> Result<(), StoreError> {
    Id::parse(kind.to_owned())?;
    Id::parse(id.to_owned())?;
    let payload = encode_payload(value)?;
    let updated = Timestamp::now()
        .to_rfc3339()
        .map_err(|error| StoreError::Timestamp(error.to_string()))?;
    transaction.execute("INSERT INTO records(kind,id,payload,updated) VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET payload=excluded.payload,updated=excluded.updated", (kind, id, payload.as_str(), updated.as_str()))?;
    if !event.is_empty() {
        insert_event_payload(transaction, event, id, &payload)?;
    }
    Ok(())
}

fn insert_event(
    transaction: &Transaction<'_>,
    kind: &str,
    id: &str,
    value: &impl Serialize,
) -> Result<(), StoreError> {
    let payload = encode_payload(value)?;
    insert_event_payload(transaction, kind, id, &payload)
}

fn insert_event_payload(
    transaction: &Transaction<'_>,
    kind: &str,
    id: &str,
    payload: &str,
) -> Result<(), StoreError> {
    let at = Timestamp::now()
        .to_rfc3339()
        .map_err(|error| StoreError::Timestamp(error.to_string()))?;
    transaction.execute(
        "INSERT INTO events(kind,ref,payload,at) VALUES(?1,?2,?3,?4)",
        (kind, id, payload, at.as_str()),
    )?;
    Ok(())
}

#[derive(Default)]
struct JsonTokenState {
    in_string: bool,
    escaped: bool,
}

#[derive(Default)]
struct WriterFailure {
    too_deep: bool,
    too_large: bool,
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    nesting: usize,
    token_state: JsonTokenState,
    failure: WriterFailure,
}

impl BoundedJsonWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            nesting: 0,
            token_state: JsonTokenState::default(),
            failure: WriterFailure::default(),
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        for byte in buffer {
            if self.token_state.in_string {
                if self.token_state.escaped {
                    self.token_state.escaped = false;
                } else if *byte == b'\\' {
                    self.token_state.escaped = true;
                } else if *byte == b'"' {
                    self.token_state.in_string = false;
                }
            } else {
                match byte {
                    b'"' => self.token_state.in_string = true,
                    b'{' | b'[' => {
                        self.nesting += 1;
                        if self.nesting > MAX_RECORD_DEPTH {
                            self.failure.too_deep = true;
                            return Err(io::Error::other("JSON nesting limit exceeded"));
                        }
                    }
                    b'}' | b']' => self.nesting = self.nesting.saturating_sub(1),
                    _ => {}
                }
            }
        }
        if buffer.len() > MAX_RECORD_BYTES.saturating_sub(self.bytes.len()) {
            self.failure.too_large = true;
            return Err(io::Error::other("JSON byte limit exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_payload(value: &impl Serialize) -> Result<String, StoreError> {
    let mut writer = BoundedJsonWriter::new();
    let serialized = stacker::grow(JSON_STACK_SEGMENT_BYTES, || {
        let mut serializer = serde_json::Serializer::new(&mut writer);
        value.serialize(serde_stacker::Serializer::new(&mut serializer))
    });
    if let Err(error) = serialized {
        if writer.failure.too_deep {
            return Err(StoreError::RecordTooDeep);
        }
        if writer.failure.too_large {
            return Err(StoreError::RecordTooLarge);
        }
        return Err(StoreError::Json(error));
    }
    let raw = String::from_utf8(writer.bytes)?;
    let mut escaped = String::with_capacity(raw.len().min(MAX_RECORD_BYTES));
    for character in raw.chars() {
        match character {
            '&' => escaped.push_str("\\u0026"),
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            _ => escaped.push(character),
        }
        if escaped.len() > MAX_RECORD_BYTES {
            return Err(StoreError::RecordTooLarge);
        }
    }
    Ok(escaped)
}

fn json_depth(payload: &str) -> Result<(), StoreError> {
    let mut nesting = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in payload.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    nesting += 1;
                    if nesting > MAX_RECORD_DEPTH {
                        return Err(StoreError::RecordTooDeep);
                    }
                }
                b'}' | b']' => nesting = nesting.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

fn decode_payload<T: DeserializeOwned>(payload: &str) -> Result<T, StoreError> {
    if payload.len() > MAX_RECORD_BYTES {
        return Err(StoreError::RecordTooLarge);
    }
    json_depth(payload)?;
    let mut deserializer = serde_json::Deserializer::from_str(payload);
    deserializer.disable_recursion_limit();
    let value = T::deserialize(serde_stacker::Deserializer::new(&mut deserializer))?;
    deserializer.end()?;
    Ok(value)
}

fn parse_payload(payload: &str) -> Result<RecordValue, StoreError> {
    decode_payload(payload).map(RecordValue::from_value)
}

/// An unsupported or internally inconsistent Rover database schema.
#[derive(Debug)]
pub enum MigrationError {
    Sqlite(rusqlite::Error),
    UnsupportedVersion(i64),
    UnversionedDatabaseHasObjects,
    LegacySchemaMismatch(String),
    LedgerMismatch,
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "SQLite migration failed: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "database schema version {version} is unsupported")
            }
            Self::UnversionedDatabaseHasObjects => {
                f.write_str("unversioned database already contains user schema objects")
            }
            Self::LegacySchemaMismatch(reason) => {
                write!(f, "legacy schema does not match Rover v1: {reason}")
            }
            Self::LedgerMismatch => f.write_str("schema migration ledger does not match Rover v1"),
        }
    }
}

impl Error for MigrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for MigrationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Apply any pending supported migrations and validate the resulting ledger.
///
/// This function does not configure `SQLite` pragmas or validate filesystem
/// paths; the eventual state-root opener owns those policies.
///
/// # Errors
///
/// Returns an error for an unsupported version, an incompatible schema, a
/// corrupted migration ledger, or a `SQLite` operation failure.
pub fn migrate(connection: &mut Connection) -> Result<(), MigrationError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => migrate_empty_database(connection),
        SCHEMA_VERSION => adopt_or_validate_v1(connection),
        future => Err(MigrationError::UnsupportedVersion(future)),
    }
}

fn migrate_empty_database(connection: &mut Connection) -> Result<(), MigrationError> {
    migrate_empty_database_with(connection, insert_ledger_entry)
}

fn migrate_empty_database_with(
    connection: &mut Connection,
    after_schema: impl FnOnce(&Connection) -> Result<(), MigrationError>,
) -> Result<(), MigrationError> {
    if has_user_schema_objects(connection)? {
        return Err(MigrationError::UnversionedDatabaseHasObjects);
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(MIGRATION_SQL)?;
    create_ledger(&transaction)?;
    after_schema(&transaction)?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    validate_v1(connection)
}

fn adopt_or_validate_v1(connection: &mut Connection) -> Result<(), MigrationError> {
    let ledger_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    validate_v1_tables(connection, ledger_exists)?;
    if ledger_exists {
        validate_ledger(connection)?;
        return Ok(());
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    create_ledger(&transaction)?;
    insert_ledger_entry(&transaction)?;
    transaction.commit()?;
    validate_v1(connection)
}

fn create_ledger(connection: &Connection) -> Result<(), MigrationError> {
    connection.execute_batch(LEDGER_SQL)?;
    Ok(())
}

fn insert_ledger_entry(connection: &Connection) -> Result<(), MigrationError> {
    connection.execute(
        "INSERT INTO schema_migrations(version,name,checksum,applied_at) \
         VALUES(?1,?2,?3,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        (SCHEMA_VERSION, MIGRATION_NAME, migration_checksum()),
    )?;
    Ok(())
}

fn validate_v1(connection: &Connection) -> Result<(), MigrationError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(MigrationError::UnsupportedVersion(version));
    }
    validate_v1_tables(connection, true)?;
    validate_ledger(connection)
}

fn validate_v1_tables(
    connection: &Connection,
    includes_ledger: bool,
) -> Result<(), MigrationError> {
    let mut expected = vec!["events", "records", "request_keys"];
    if includes_ledger {
        expected.push("schema_migrations");
    }
    expected.sort_unstable();

    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master \
         WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let actual: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if actual != expected {
        return Err(MigrationError::LegacySchemaMismatch(
            "unexpected table set".to_string(),
        ));
    }

    validate_columns(
        connection,
        "records",
        &[
            ("kind", "TEXT", 1, 1),
            ("id", "TEXT", 1, 2),
            ("payload", "TEXT", 1, 0),
            ("updated", "TEXT", 1, 0),
        ],
    )?;
    validate_columns(
        connection,
        "events",
        &[
            ("seq", "INTEGER", 0, 1),
            ("kind", "TEXT", 1, 0),
            ("ref", "TEXT", 1, 0),
            ("payload", "TEXT", 1, 0),
            ("at", "TEXT", 1, 0),
        ],
    )?;
    validate_columns(
        connection,
        "request_keys",
        &[
            ("key", "TEXT", 0, 1),
            ("digest", "TEXT", 1, 0),
            ("ref", "TEXT", 1, 0),
        ],
    )?;
    if includes_ledger {
        validate_columns(
            connection,
            "schema_migrations",
            &[
                ("version", "INTEGER", 0, 1),
                ("name", "TEXT", 1, 0),
                ("checksum", "TEXT", 1, 0),
                ("applied_at", "TEXT", 1, 0),
            ],
        )?;
        let ledger_sql: String = connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get(0),
        )?;
        if normalize_sql(&ledger_sql) != normalize_sql(LEDGER_SQL.trim_end_matches(';')) {
            return Err(MigrationError::LegacySchemaMismatch(
                "migration ledger constraints mismatch".to_string(),
            ));
        }
    }

    let events_sql: String = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='events'",
        [],
        |row| row.get(0),
    )?;
    if !events_sql.to_ascii_uppercase().contains("AUTOINCREMENT") {
        return Err(MigrationError::LegacySchemaMismatch(
            "events sequence is not AUTOINCREMENT".to_string(),
        ));
    }

    let unexpected_objects: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master \
         WHERE type IN ('index','trigger','view') AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    if unexpected_objects != 0 {
        return Err(MigrationError::LegacySchemaMismatch(
            "unexpected index, trigger, or view".to_string(),
        ));
    }
    Ok(())
}

fn validate_columns(
    connection: &Connection,
    table: &str,
    expected: &[(&str, &str, i64, i64)],
) -> Result<(), MigrationError> {
    let sql = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&sql)?;
    let actual: Vec<(String, String, i64, i64)> = statement
        .query_map([], |row| {
            Ok((row.get(1)?, row.get(2)?, row.get(3)?, row.get(5)?))
        })?
        .collect::<Result<_, _>>()?;
    let expected: Vec<(String, String, i64, i64)> = expected
        .iter()
        .map(|(name, kind, not_null, primary_key)| {
            (
                (*name).to_string(),
                (*kind).to_string(),
                *not_null,
                *primary_key,
            )
        })
        .collect();
    if actual != expected {
        return Err(MigrationError::LegacySchemaMismatch(format!(
            "column definition mismatch in {table}: actual={actual:?}, expected={expected:?}"
        )));
    }
    Ok(())
}

fn validate_ledger(connection: &Connection) -> Result<(), MigrationError> {
    let rows: Vec<(i64, String, String, String)> = {
        let mut statement = connection.prepare(
            "SELECT version,name,checksum,applied_at FROM schema_migrations ORDER BY version",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    if rows.len() != 1
        || rows[0].0 != SCHEMA_VERSION
        || rows[0].1 != MIGRATION_NAME
        || rows[0].2 != migration_checksum()
        || !looks_like_utc_timestamp(&rows[0].3)
    {
        return Err(MigrationError::LedgerMismatch);
    }
    Ok(())
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
        .replace(" (", "(")
        .replace(" )", ")")
        .replace(" = ", "=")
        .replace(" > ", ">")
        .replace(", ", ",")
}

fn has_user_schema_objects(connection: &Connection) -> Result<bool, MigrationError> {
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master \
         WHERE name NOT LIKE 'sqlite_%' AND type IN ('table','index','trigger','view')",
        [],
        |row| row.get(0),
    )?;
    Ok(count != 0)
}

fn migration_checksum() -> String {
    let digest = Sha256::digest(MIGRATION_SQL.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn looks_like_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b':'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[16] == b':'
        && bytes[17..19].iter().all(u8::is_ascii_digit)
        && bytes[19] == b'.'
        && bytes[20..23].iter().all(u8::is_ascii_digit)
        && bytes[23] == b'Z'
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempDb(std::path::PathBuf);
    impl TempDb {
        fn new() -> Self {
            let unique = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            Self(
                std::env::temp_dir()
                    .join(format!("rover-store-{}-{unique}.db", std::process::id())),
            )
        }
        fn store(&self) -> Store {
            Store::from_connection(Connection::open(&self.0).expect("open temp DB"))
                .expect("configure and migrate")
        }
    }
    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(self.0.with_extension("db-wal"));
            let _ = fs::remove_file(self.0.with_extension("db-shm"));
        }
    }

    fn in_memory_store() -> Store {
        Store::from_connection(Connection::open_in_memory().expect("open memory DB"))
            .expect("configure and migrate")
    }

    #[test]
    fn record_value_reads_nested_string_fields_without_json_type_exposure() {
        let record = RecordValue::from_value(serde_json::json!({
            "id": "task-1",
            "contract": {"objective": "inspect task"},
            "process": {"exit_code": 7, "timed_out": false},
            "attempts": [{"number": 1}, {"number": 2}]
        }));
        assert_eq!(record.string_at_path(&["id"]), Some("task-1"));
        assert_eq!(
            record.string_at_path(&["contract", "objective"]),
            Some("inspect task")
        );
        assert_eq!(record.string_at_path(&["contract", "missing"]), None);
        assert_eq!(record.string_at_path(&[]), None);
        assert_eq!(record.integer_at_path(&["process", "exit_code"]), Some(7));
        assert_eq!(record.bool_at_path(&["process", "timed_out"]), Some(false));
        assert_eq!(record.array_len_at_path(&["attempts"]), Some(2));
        assert_eq!(
            record.array_values_at_path(&["attempts"]).map(<[_]>::len),
            Some(2)
        );
        assert!(record.contains_path(&["process", "timed_out"]));
        assert!(!record.contains_path(&["process", "missing"]));
    }

    #[test]
    fn put_get_list_events_and_delete_match_record_contract() {
        let store = in_memory_store();
        let value = serde_json::json!({"text": "<>&\u{2028}\u{2029}", "n": 3});
        store
            .put("task", "one", &value, "task.created")
            .expect("put");
        let stored = store.get("task", "one").expect("get");
        assert_eq!(stored.as_value(), &value);
        let listed = store.list("task", 10).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].as_value(), &value);
        let events = store.events("one").expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["sequence"], "1");
        assert_eq!(events[0]["event"], "task.created");
        assert!(events[0].get("payload").is_none());

        store.delete("task", "one", "task.deleted").expect("delete");
        assert!(matches!(
            store.get("task", "one"),
            Err(StoreError::NotFound)
        ));
        let events = store.events("one").expect("events after delete");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["event"], "task.deleted");
        assert!(matches!(
            store.delete("task", "one", ""),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn event_summaries_stop_at_the_go_limit_of_one_thousand() {
        let store = in_memory_store();
        let mut connection = store.lock().expect("connection");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("transaction");
        for _ in 0..1001 {
            transaction
                .execute(
                    "INSERT INTO events(kind,ref,payload,at) VALUES('changed','many','{}','2026-09-25T00:00:00Z')",
                    [],
                )
                .expect("event insert");
        }
        transaction.commit().expect("commit events");
        drop(connection);
        let summaries = store.events("many").expect("bounded event summaries");
        assert_eq!(summaries.len(), 1000);
        assert_eq!(summaries[0]["sequence"], "1");
        assert_eq!(summaries[999]["sequence"], "1000");
    }

    #[test]
    fn put_serializes_once_and_uses_identical_record_and_event_payloads() {
        struct ChangingValue(AtomicU64);
        impl Serialize for ChangingValue {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u64(self.0.fetch_add(1, Ordering::Relaxed))
            }
        }

        let store = in_memory_store();
        let value = ChangingValue(AtomicU64::new(0));
        store
            .put("task", "one", &value, "task.created")
            .expect("put");
        let connection = store.lock().expect("connection");
        let (record_payload, event_payload): (String, String) = connection
            .query_row(
                "SELECT records.payload,events.payload FROM records JOIN events ON events.ref=records.id WHERE records.id='one'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("record and event payloads");
        assert_eq!(record_payload, "0");
        assert_eq!(event_payload, record_payload);
        assert_eq!(value.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn mutate_is_atomic_on_callback_error_and_success() {
        let store = in_memory_store();
        store
            .put("task", "one", &serde_json::json!({"count": 1}), "")
            .expect("initial put");
        let error = store.mutate("task", "one", "task.changed", |_| {
            Err(StoreError::InvalidBound)
        });
        assert!(matches!(error, Err(StoreError::InvalidBound)));
        assert_eq!(
            store.get("task", "one").expect("unchanged record")["count"],
            1
        );
        assert!(store.events("one").expect("no partial event").is_empty());

        store
            .mutate("task", "one", "task.changed", |old| {
                Ok(serde_json::json!({"count": old["count"].as_i64().unwrap() + 1}))
            })
            .expect("mutate");
        assert_eq!(
            store.get("task", "one").expect("updated record")["count"],
            2
        );
        assert_eq!(store.events("one").expect("event").len(), 1);
    }

    #[test]
    fn concurrent_mutations_do_not_lose_updates() {
        let store = Arc::new(in_memory_store());
        store
            .put("counter", "shared", &serde_json::json!({"count":0}), "")
            .expect("initial counter");
        let workers = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    for _ in 0..25 {
                        store
                            .mutate("counter", "shared", "", |old| {
                                Ok(serde_json::json!({"count": old["count"].as_i64().unwrap() + 1}))
                            })
                            .expect("atomic increment");
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("mutation worker");
        }
        assert_eq!(
            store.get("counter", "shared").expect("counter read")["count"],
            200
        );
    }

    #[test]
    fn panic_during_mutation_rolls_back_and_reopens_cleanly() {
        let database = TempDb::new();
        {
            let store = database.store();
            store
                .put("task", "one", &serde_json::json!({"count":1}), "")
                .expect("initial record");
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = store.mutate(
                    "task",
                    "one",
                    "task.changed",
                    |_| -> Result<_, StoreError> { panic!("injected mutation panic") },
                );
            }));
            assert!(result.is_err());
            assert!(matches!(
                store.get("task", "one"),
                Err(StoreError::Poisoned)
            ));
        }
        let reopened = database.store();
        assert_eq!(
            reopened.get("task", "one").expect("record after recovery")["count"],
            1
        );
        assert!(reopened
            .events("one")
            .expect("no uncommitted event")
            .is_empty());
    }

    #[test]
    fn malformed_stored_json_is_reported_without_repair() {
        let store = in_memory_store();
        store
            .lock()
            .expect("connection")
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES('task','bad','{','2026-09-25T00:00:00Z')",
                [],
            )
            .expect("inject malformed legacy payload");
        assert!(matches!(store.get("task", "bad"), Err(StoreError::Json(_))));
        let raw: String = store
            .lock()
            .expect("connection")
            .query_row("SELECT payload FROM records WHERE id='bad'", [], |row| {
                row.get(0)
            })
            .expect("payload remains untouched");
        assert_eq!(raw, "{");
    }

    #[test]
    fn create_once_rejects_changed_digest_without_partial_write_and_empty_key_skips_key_table() {
        let store = in_memory_store();
        assert_eq!(
            store
                .create_once(
                    "task",
                    "first",
                    "key",
                    "digest-a",
                    &serde_json::json!({"x":1})
                )
                .expect("first create"),
            ("first".to_owned(), false)
        );
        assert_eq!(
            store
                .create_once(
                    "task",
                    "second",
                    "key",
                    "digest-a",
                    &serde_json::json!({"x":2})
                )
                .expect("repeat create"),
            ("first".to_owned(), true)
        );
        assert!(matches!(
            store.create_once("task", "third", "key", "digest-b", &serde_json::json!({})),
            Err(StoreError::IdempotencyConflict)
        ));
        assert!(matches!(
            store.get("task", "third"),
            Err(StoreError::NotFound)
        ));
        assert_eq!(store.events("first").expect("single event").len(), 1);
        store
            .create_once("task", "no-key", "", "ignored", &serde_json::json!({}))
            .expect("create without idempotency");
        let connection = store.lock().expect("connection lock");
        let keys: i64 = connection
            .query_row("SELECT count(*) FROM request_keys", [], |row| row.get(0))
            .expect("key count");
        assert_eq!(keys, 1);
    }

    #[test]
    fn typed_reads_and_raw_mutation_preserve_caller_selected_number_semantics() {
        let store = in_memory_store();
        let exact = u64::MAX;
        store
            .put("number", "large", &exact, "")
            .expect("put unsigned integer");
        let decoded: u64 = store.get_typed("number", "large").expect("typed read");
        assert_eq!(decoded, exact);
        assert_eq!(
            store.list_raw("number", 10).expect("raw list"),
            vec![u64::MAX.to_string()]
        );
        assert_eq!(
            store.list_all_raw("number", 10).expect("raw all list"),
            vec![u64::MAX.to_string()]
        );

        store
            .mutate_raw::<u64, _>("number", "large", "number.changed", |payload| {
                let old: u64 = serde_json::from_str(payload)?;
                Ok(old - 1)
            })
            .expect("raw typed mutation");
        let updated: u64 = store
            .get_typed("number", "large")
            .expect("updated typed read");
        assert_eq!(updated, exact - 1);
    }

    #[test]
    fn create_once_is_idempotent_across_independent_connections() {
        let database = TempDb::new();
        let first = Arc::new(database.store());
        let second = Arc::new(database.store());
        let barrier = Arc::new(Barrier::new(3));
        let workers = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, store)| {
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    store
                        .create_once(
                            "task",
                            &format!("id-{index}"),
                            "shared-key",
                            "same-digest",
                            &serde_json::json!({"index":index}),
                        )
                        .expect("create once")
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .collect::<Vec<_>>();
        assert_eq!(results[0].0, results[1].0);
        assert_eq!(results.iter().filter(|(_, exists)| !exists).count(), 1);
        assert_eq!(
            database
                .store()
                .events(&results[0].0)
                .expect("one durable event")
                .len(),
            1
        );
    }

    #[test]
    fn record_event_and_idempotency_key_roll_back_as_one_unit() {
        let store = in_memory_store();
        store
            .lock()
            .expect("connection")
            .execute_batch(
                "CREATE TRIGGER reject_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected event failure'); END;",
            )
            .expect("install event failure trigger");
        assert!(matches!(
            store.create_once(
                "task",
                "atomic",
                "atomic-key",
                "digest",
                &serde_json::json!({})
            ),
            Err(StoreError::Sqlite(_))
        ));
        store
            .lock()
            .expect("connection")
            .execute_batch("DROP TRIGGER reject_event;")
            .expect("remove failure trigger");
        assert!(matches!(
            store.get("task", "atomic"),
            Err(StoreError::NotFound)
        ));
        let connection = store.lock().expect("connection");
        let (event_count, key_count): (i64, i64) = connection
            .query_row(
                "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM request_keys)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read rolled back rows");
        assert_eq!((event_count, key_count), (0, 0));
        drop(connection);

        store
            .put("task", "delete-atomic", &serde_json::json!({}), "")
            .expect("record before delete");
        store
            .lock()
            .expect("connection")
            .execute_batch(
                "CREATE TRIGGER reject_delete_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected delete event failure'); END;",
            )
            .expect("install delete failure trigger");
        assert!(matches!(
            store.delete("task", "delete-atomic", "task.deleted"),
            Err(StoreError::Sqlite(_))
        ));
        store
            .lock()
            .expect("connection")
            .execute_batch("DROP TRIGGER reject_delete_event;")
            .expect("remove delete failure trigger");
        assert!(store.get("task", "delete-atomic").is_ok());
    }

    #[test]
    fn records_survive_connection_close_and_reopen() {
        let database = TempDb::new();
        {
            let store = database.store();
            store
                .put(
                    "task",
                    "persisted",
                    &serde_json::json!({"ok":true}),
                    "task.created",
                )
                .expect("persist");
        }
        let reopened = database.store();
        assert_eq!(
            reopened
                .get("task", "persisted")
                .expect("read after reopen")["ok"],
            true
        );
        assert_eq!(
            reopened
                .events("persisted")
                .expect("event after reopen")
                .len(),
            1
        );
    }

    #[test]
    fn connection_pragmas_match_go_store_contract() {
        let database = TempDb::new();
        let store = database.store();
        let connection = store.lock().expect("connection");
        let (journal_mode, synchronous, foreign_keys, busy_timeout): (String, i64, i64, i64) =
            connection
                .query_row(
                    "SELECT (SELECT * FROM pragma_journal_mode),(SELECT * FROM pragma_synchronous),(SELECT * FROM pragma_foreign_keys),(SELECT * FROM pragma_busy_timeout)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .expect("read configured pragmas");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!((synchronous, foreign_keys, busy_timeout), (2, 1, 5000));
    }

    #[test]
    fn payload_json_uses_go_escaping_and_enforces_encoded_size_limit() {
        let encoded = encode_payload(&serde_json::json!("<>&\u{2028}\u{2029}")).expect("encode");
        assert!(encoded.contains("\\u003c\\u003e\\u0026\\u2028\\u2029"));
        let object = serde_json::json!({"z": 1, "a": 2});
        assert_eq!(
            encode_payload(&object).expect("sorted object keys"),
            r#"{"a":2,"z":1}"#
        );
        let too_large = serde_json::json!("x".repeat(MAX_RECORD_BYTES));
        assert!(matches!(
            encode_payload(&too_large),
            Err(StoreError::RecordTooLarge)
        ));

        let make_nested = |levels: usize| {
            let mut value = serde_json::Value::Null;
            for _ in 0..levels {
                value = serde_json::Value::Array(vec![value]);
            }
            RecordValue::from_value(value)
        };
        let at_limit = make_nested(MAX_RECORD_DEPTH);
        let encoded_at_limit = encode_payload(at_limit.as_value()).expect("Go maximum depth");
        let decoded_at_limit = parse_payload(&encoded_at_limit).expect("parse Go maximum depth");
        let mut leaf = decoded_at_limit.as_value();
        for _ in 0..MAX_RECORD_DEPTH {
            leaf = &leaf[0];
        }
        assert!(leaf.is_null());

        let mut nested_object_value = serde_json::Value::Null;
        for _ in 0..MAX_RECORD_DEPTH {
            let mut object = serde_json::Map::new();
            object.insert("x".to_owned(), nested_object_value);
            nested_object_value = serde_json::Value::Object(object);
        }
        let nested_object_value = RecordValue::from_value(nested_object_value);
        let encoded_object = encode_payload(nested_object_value.as_value())
            .expect("serialize deeply nested objects");
        assert!(encoded_object.starts_with(r#"{"x":{"x":"#));

        let deep_object_json = format!(
            "{}0{}",
            r#"{"x":"#.repeat(MAX_RECORD_DEPTH),
            "}".repeat(MAX_RECORD_DEPTH)
        );
        let decoded_object = parse_payload(&deep_object_json).expect("parse deeply nested objects");
        let mut leaf = decoded_object.as_value();
        for _ in 0..MAX_RECORD_DEPTH {
            leaf = &leaf["x"];
        }
        assert!(leaf.is_number());

        let too_deep = make_nested(MAX_RECORD_DEPTH + 1);
        assert!(matches!(
            encode_payload(too_deep.as_value()),
            Err(StoreError::RecordTooDeep)
        ));
        let too_deep_json = format!(
            "{}0{}",
            "[".repeat(MAX_RECORD_DEPTH + 1),
            "]".repeat(MAX_RECORD_DEPTH + 1)
        );
        assert!(matches!(
            parse_payload(&too_deep_json),
            Err(StoreError::RecordTooDeep)
        ));
    }

    #[test]
    fn invalid_ids_and_explicit_list_bounds_fail_closed() {
        let store = in_memory_store();
        assert!(matches!(
            store.put("bad kind", "one", &serde_json::json!({}), ""),
            Err(StoreError::InvalidId(_))
        ));
        assert!(matches!(
            store.list_all("task", 0),
            Err(StoreError::InvalidBound)
        ));
        store
            .put("task", "a", &serde_json::json!({}), "")
            .expect("record a");
        store
            .put("task", "b", &serde_json::json!({}), "")
            .expect("record b");
        assert!(matches!(
            store.list_all("task", 1),
            Err(StoreError::CollectionTooLarge)
        ));
        assert_eq!(store.list("task", 0).expect("default list limit").len(), 2);
    }

    fn empty_database() -> Connection {
        Connection::open_in_memory().expect("open in-memory database")
    }

    fn create_legacy_v1(connection: &Connection) {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id));\
                 CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL);\
                 CREATE TABLE IF NOT EXISTS request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL);",
            )
            .expect("create schema from Go v1 store");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .expect("set legacy schema version");
    }

    #[test]
    fn fresh_database_gets_v1_tables_and_a_hashed_ledger_entry() {
        let mut connection = empty_database();
        migrate(&mut connection).expect("initial migration");
        validate_v1(&connection).expect("valid resulting schema");

        let ledger: (i64, String, String, String) = connection
            .query_row(
                "SELECT version,name,checksum,applied_at FROM schema_migrations",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("ledger row");
        assert_eq!(ledger.0, 1);
        assert_eq!(ledger.1, MIGRATION_NAME);
        assert_eq!(ledger.2, migration_checksum());
        assert!(looks_like_utc_timestamp(&ledger.3));
    }

    #[test]
    fn running_migrations_again_is_idempotent_and_preserves_records() {
        let mut connection = empty_database();
        migrate(&mut connection).expect("initial migration");
        connection
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES('task','one','{}','2026-09-25T00:00:00Z')",
                [],
            )
            .expect("insert representative record");

        migrate(&mut connection).expect("repeat migration");

        let (record_count, ledger_count): (i64, i64) = connection
            .query_row(
                "SELECT (SELECT count(*) FROM records),(SELECT count(*) FROM schema_migrations)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read row counts");
        assert_eq!((record_count, ledger_count), (1, 1));
    }

    #[test]
    fn legacy_v1_database_is_adopted_without_changing_its_data() {
        let mut connection = empty_database();
        create_legacy_v1(&connection);
        connection
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES(?1,?2,?3,?4)",
                params![
                    "investigation",
                    "legacy-1",
                    "{\"kept\":true}",
                    "2026-09-25T00:00:00Z"
                ],
            )
            .expect("insert legacy record");
        connection
            .execute(
                "INSERT INTO events(kind,ref,payload,at) VALUES(?1,?2,?3,?4)",
                params!["created", "legacy-1", "{}", "2026-09-25T00:00:00Z"],
            )
            .expect("insert legacy event");
        connection
            .execute(
                "INSERT INTO request_keys(key,digest,ref) VALUES(?1,?2,?3)",
                params!["request-1", "digest-1", "legacy-1"],
            )
            .expect("insert legacy idempotency key");

        migrate(&mut connection).expect("adopt compatible legacy database");

        let record: String = connection
            .query_row(
                "SELECT payload FROM records WHERE id='legacy-1'",
                [],
                |row| row.get(0),
            )
            .expect("legacy record remains");
        let (events, request_keys, ledger): (i64, i64, i64) = connection
            .query_row(
                "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM request_keys),(SELECT count(*) FROM schema_migrations)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("legacy event, key, and new ledger remain");
        assert_eq!(record, "{\"kept\":true}");
        assert_eq!((events, request_keys, ledger), (1, 1, 1));
    }

    #[test]
    fn newer_schema_is_rejected_without_modification() {
        let mut connection = empty_database();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("set future version");
        let error = migrate(&mut connection).expect_err("future schema is unsupported");
        assert!(matches!(error, MigrationError::UnsupportedVersion(2)));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read unchanged version");
        assert_eq!(version, 2);
    }

    #[test]
    fn nonempty_unversioned_database_is_refused_without_table_changes() {
        let mut connection = empty_database();
        connection
            .execute_batch("CREATE TABLE unrelated(value TEXT);")
            .expect("create unversioned table");
        let error = migrate(&mut connection).expect_err("unversioned data is ambiguous");
        assert!(matches!(
            error,
            MigrationError::UnversionedDatabaseHasObjects
        ));
        let exists: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='unrelated'",
                [],
                |row| row.get(0),
            )
            .expect("query unchanged table");
        assert_eq!(exists, 1);
    }

    #[test]
    fn failed_initial_migration_rolls_back_all_schema_changes() {
        let mut connection = empty_database();
        let result = migrate_empty_database_with(&mut connection, |transaction| {
            transaction.execute_batch("THIS IS NOT SQL")?;
            Ok(())
        });
        assert!(result.is_err());

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version remains readable");
        assert_eq!(version, 0);
        assert!(!has_user_schema_objects(&connection).expect("query rolled-back schema"));
    }

    #[test]
    fn v1_database_with_wrong_ledger_checksum_is_refused() {
        let mut connection = empty_database();
        create_legacy_v1(&connection);
        create_ledger(&connection).expect("create migration ledger");
        connection
            .execute(
                "INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(1,?1,?2,'2026-09-25T00:00:00.000Z')",
                params![MIGRATION_NAME, "0".repeat(64)],
            )
            .expect("insert invalid ledger entry");

        let error = migrate(&mut connection).expect_err("changed migration checksum is unsafe");
        assert!(matches!(error, MigrationError::LedgerMismatch));
    }

    #[test]
    fn legacy_v1_schema_with_changed_event_sequence_is_refused() {
        let mut connection = empty_database();
        connection
            .execute_batch(
                "CREATE TABLE records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id));\
                 CREATE TABLE events (seq INTEGER PRIMARY KEY,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL);\
                 CREATE TABLE request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL);\
                 PRAGMA user_version=1;",
            )
            .expect("create altered legacy schema");

        let error =
            migrate(&mut connection).expect_err("changed sequence semantics are incompatible");

        assert!(matches!(error, MigrationError::LegacySchemaMismatch(_)));
        let ledger_exists: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                [],
                |row| row.get(0),
            )
            .expect("check that failed adoption wrote no ledger");
        assert_eq!(ledger_exists, 0);
    }
}
