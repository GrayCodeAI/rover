//! Idempotent import of Go Rover state into a new Rust state root.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use rover_core::{Id, Sha256Digest, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    canonical_state_path, combined_digest, hex_digest, inspect_events, inspect_objects,
    inspect_records, inspect_request_keys, update_field, LegacySchema, LegacyStateInventory,
};
use crate::files::{SafeDir, StateRoot};
use crate::{insert_event_payload, put_in_transaction, StoreError};

const IMPORT_KIND: &str = "migration";
const IMPORT_EVENT: &str = "migration.imported";
const SANITIZE_EVENT: &str = "record.migration_sanitized";
const SANITIZE_ERROR: &str =
    "imported Go state; process state was not restored; no automatic execution";

/// Explicit assertions required before import.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LegacyImportOptions {
    /// The operator has stopped Rover and every worker using the source state.
    pub source_quiesced: bool,
    /// The operator accepts that task/check trees and unmanaged root files are
    /// inventoried but are not copied into the new state.
    pub accept_excluded_runtime_data: bool,
}

/// Durable import receipt stored in the destination state root.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyImportReceipt {
    /// Receipt schema version.
    pub schema: String,
    /// SHA-256 of the canonical source path, also used as the receipt ID.
    pub source_root_digest: String,
    /// Digest of source records, events, request keys, and verified objects.
    pub source_digest: String,
    /// Source records copied, excluding released leases.
    pub records: u64,
    /// Source events copied before migration sanitation events.
    pub events: u64,
    /// Source request keys copied.
    pub request_keys: u64,
    /// Verified content-addressed objects copied.
    pub objects: u64,
    /// Grant records forced to revoked in the destination.
    pub grants_revoked: u64,
    /// Lease rows omitted and released during import.
    pub leases_released: u64,
    /// Nonterminal task/workflow records marked LOST.
    pub executions_marked_lost: u64,
    /// Number of records sanitized or lease rows released.
    pub sanitized_records: u64,
    /// RFC3339 import completion timestamp.
    pub imported_at: String,
}

/// Result of one import attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyImportReport {
    /// Durable receipt describing the source snapshot.
    pub receipt: LegacyImportReceipt,
    /// True when this exact source snapshot had already been imported.
    pub already_imported: bool,
}

/// A fail-closed legacy import error.
#[derive(Debug)]
pub enum LegacyImportError {
    SourceNotQuiesced,
    ExcludedData(Vec<String>),
    SourceBlocked(Vec<String>),
    PathsOverlap,
    DestinationNotNew,
    SourceChanged,
    InvalidRecord(String),
    InvalidReceipt,
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Store(StoreError),
    Json(serde_json::Error),
    Timestamp(String),
}

impl fmt::Display for LegacyImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceNotQuiesced => formatter.write_str("source must be quiesced before import"),
            Self::ExcludedData(items) => write!(
                formatter,
                "runtime or unmanaged source data requires explicit exclusion consent: {}",
                items.join(", ")
            ),
            Self::SourceBlocked(blockers) => write!(
                formatter,
                "source inventory blocks import: {}",
                blockers.join("; ")
            ),
            Self::PathsOverlap => formatter.write_str("source and destination state trees overlap"),
            Self::DestinationNotNew => formatter.write_str(
                "destination is not empty, resumable, or already imported from this exact source",
            ),
            Self::SourceChanged => formatter.write_str("source changed during migration"),
            Self::InvalidRecord(reason) => write!(formatter, "unsafe legacy record: {reason}"),
            Self::InvalidReceipt => formatter
                .write_str("migration receipt is invalid or belongs to another source snapshot"),
            Self::Io(error) => write!(
                formatter,
                "legacy import filesystem operation failed: {error}"
            ),
            Self::Sqlite(error) => {
                write!(formatter, "legacy import SQLite operation failed: {error}")
            }
            Self::Store(error) => {
                write!(formatter, "legacy import store operation failed: {error}")
            }
            Self::Json(error) => write!(formatter, "legacy import JSON operation failed: {error}"),
            Self::Timestamp(error) => write!(formatter, "legacy import timestamp failed: {error}"),
        }
    }
}

impl Error for LegacyImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::SourceNotQuiesced
            | Self::ExcludedData(_)
            | Self::SourceBlocked(_)
            | Self::PathsOverlap
            | Self::DestinationNotNew
            | Self::SourceChanged
            | Self::InvalidRecord(_)
            | Self::InvalidReceipt
            | Self::Timestamp(_) => None,
        }
    }
}

impl From<io::Error> for LegacyImportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for LegacyImportError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreError> for LegacyImportError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<serde_json::Error> for LegacyImportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl LegacyStateInventory {
    /// Import source rows and verified objects into a distinct new Rust state.
    ///
    /// Repeating the import verifies and returns its stored receipt without
    /// duplicating database rows, events, request keys, or objects. The source
    /// is only opened read-only. Grants are revoked, leases released, and
    /// task/workflow process metadata sanitized to match safe restore behavior.
    ///
    /// # Errors
    ///
    /// Returns an error if the operator does not assert source quiescence,
    /// excluded runtime files are not acknowledged, the inventory has blockers,
    /// source and destination overlap, or the destination is unrelated.
    pub fn import_to(
        source_root: impl AsRef<Path>,
        destination_root: impl AsRef<Path>,
        options: LegacyImportOptions,
    ) -> Result<LegacyImportReport, LegacyImportError> {
        if !options.source_quiesced {
            return Err(LegacyImportError::SourceNotQuiesced);
        }
        let inventory = Self::inspect(source_root)?;
        if !inventory
            .dry_run
            .database_and_objects_importable_after_quiesce
        {
            return Err(LegacyImportError::SourceBlocked(
                inventory.dry_run.blockers.clone(),
            ));
        }
        let excluded = excluded_data(&inventory);
        if !excluded.is_empty() && !options.accept_excluded_runtime_data {
            return Err(LegacyImportError::ExcludedData(excluded));
        }

        let source_root = inventory.root.clone();
        let destination_root = canonical_state_path(destination_root.as_ref())?;
        if paths_overlap(&source_root, &destination_root) {
            return Err(LegacyImportError::PathsOverlap);
        }
        let source_root_digest =
            Sha256Digest::of(source_root.as_os_str().as_encoded_bytes()).to_hex();
        let source_database = source_root.join("rover.db");
        let mut source_connection = Connection::open_with_flags(
            &source_database,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )?;
        source_connection.execute_batch("PRAGMA query_only=ON")?;
        let source_transaction = source_connection.transaction()?;
        validate_source_snapshot(&source_transaction, &source_root, &inventory)?;
        let source_objects = object_names(&source_root)?;
        if let Some(receipt) = preflight_destination(
            &destination_root,
            &source_root_digest,
            &inventory.dry_run.source_digest,
            &source_objects,
            &source_transaction,
            &inventory,
        )? {
            return Ok(LegacyImportReport {
                receipt,
                already_imported: true,
            });
        }

        let destination = StateRoot::open(&destination_root)?;
        let object_count = copy_objects(&source_root, &destination, &inventory)?;
        if object_count != inventory.objects.verified {
            return Err(LegacyImportError::SourceChanged);
        }

        let mut destination_connection = destination.records().lock()?;
        let transaction =
            destination_connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_destination_empty(&transaction)?;
        let counts = if inventory.user_version == 0 {
            RecordImportCounts::default()
        } else {
            copy_events(&source_transaction, &transaction)?;
            let counts = copy_records(&source_transaction, &transaction)?;
            copy_request_keys(&source_transaction, &transaction)?;
            counts
        };

        // Refuse to commit the SQL snapshot if the source changed while its
        // filesystem objects were copied.
        let current = Self::inspect(&source_root)?;
        if current.dry_run.source_digest != inventory.dry_run.source_digest {
            return Err(LegacyImportError::SourceChanged);
        }

        let receipt = new_import_receipt(&source_root_digest, &inventory, object_count, &counts)?;
        let receipt_key_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE kind=?1 AND id=?2)",
            (IMPORT_KIND, &receipt.source_root_digest),
            |row| row.get(0),
        )?;
        if receipt_key_exists {
            return Err(LegacyImportError::InvalidRecord(
                "source already contains the destination receipt key".to_owned(),
            ));
        }
        put_in_transaction(
            &transaction,
            IMPORT_KIND,
            &receipt.source_root_digest,
            &receipt,
            IMPORT_EVENT,
        )?;
        transaction.commit()?;
        source_transaction.commit()?;
        Ok(LegacyImportReport {
            receipt,
            already_imported: false,
        })
    }
}

#[derive(Default)]
struct RecordImportCounts {
    records: u64,
    grants_revoked: u64,
    leases_released: u64,
    executions_marked_lost: u64,
    sanitized_records: u64,
}

fn new_import_receipt(
    source_root_digest: &str,
    inventory: &LegacyStateInventory,
    object_count: u64,
    counts: &RecordImportCounts,
) -> Result<LegacyImportReceipt, LegacyImportError> {
    Ok(LegacyImportReceipt {
        schema: "rover-migration/v1".to_owned(),
        source_root_digest: source_root_digest.to_owned(),
        source_digest: inventory.dry_run.source_digest.clone(),
        records: counts.records,
        events: inventory.events.rows,
        request_keys: inventory.request_keys.rows,
        objects: object_count,
        grants_revoked: counts.grants_revoked,
        leases_released: counts.leases_released,
        executions_marked_lost: counts.executions_marked_lost,
        sanitized_records: counts.sanitized_records,
        imported_at: Timestamp::now()
            .to_rfc3339()
            .map_err(|error| LegacyImportError::Timestamp(error.to_string()))?,
    })
}

fn excluded_data(inventory: &LegacyStateInventory) -> Vec<String> {
    let mut excluded = Vec::new();
    if tree_nonempty(&inventory.task_tree) {
        excluded.push("tasks/".to_owned());
    }
    if tree_nonempty(&inventory.check_tree) {
        excluded.push("checks/".to_owned());
    }
    excluded.extend(inventory.unmanaged_root_entries.iter().cloned());
    excluded
}

fn tree_nonempty(tree: &super::RuntimeTreeInventory) -> bool {
    tree.files > 0 || tree.directories > 0 || tree.symlinks > 0 || tree.other_entries > 0
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn preflight_destination(
    root: &Path,
    source_root_digest: &str,
    source_digest: &str,
    source_objects: &[String],
    source: &Transaction<'_>,
    inventory: &LegacyStateInventory,
) -> Result<Option<LegacyImportReceipt>, LegacyImportError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(LegacyImportError::DestinationNotNew);
    }
    let entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    if entries.is_empty() {
        return Ok(None);
    }
    let database_path = root.join("rover.db");
    match fs::symlink_metadata(&database_path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        _ => return Err(LegacyImportError::DestinationNotNew),
    }
    let _root = SafeDir::open(root)?;
    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only=ON")?;
    crate::validate_v1(&connection).map_err(|_| LegacyImportError::DestinationNotNew)?;

    let marker = connection
        .query_row(
            "SELECT payload FROM records WHERE kind=?1 AND id=?2",
            (IMPORT_KIND, source_root_digest),
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(marker) = marker {
        let receipt: LegacyImportReceipt = serde_json::from_str(&marker)?;
        if receipt.schema != "rover-migration/v1"
            || receipt.source_root_digest != source_root_digest
            || receipt.source_digest != source_digest
        {
            return Err(LegacyImportError::InvalidReceipt);
        }
        verify_receipt_destination(
            source,
            &connection,
            root,
            &receipt,
            source_objects,
            inventory,
        )?;
        return Ok(Some(receipt));
    }

    let (records, events, request_keys): (i64, i64, i64) = connection.query_row(
        "SELECT (SELECT count(*) FROM records),(SELECT count(*) FROM events),(SELECT count(*) FROM request_keys)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if records != 0 || events != 0 || request_keys != 0 {
        return Err(LegacyImportError::DestinationNotNew);
    }

    ensure_resumable_destination(root, &entries, source_objects)?;
    Ok(None)
}

fn ensure_resumable_destination(
    root: &Path,
    entries: &[fs::DirEntry],
    source_objects: &[String],
) -> Result<(), LegacyImportError> {
    for folder in ["tasks", "checks"] {
        let path = root.join(folder);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if fs::read_dir(path)?.next().transpose()?.is_some() {
                    return Err(LegacyImportError::DestinationNotNew);
                }
            }
            Ok(_) => return Err(LegacyImportError::DestinationNotNew),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let known = [
        "rover.db",
        "rover.db-wal",
        "rover.db-shm",
        "rover.db-journal",
        "objects",
        "tasks",
        "checks",
    ];
    if entries.iter().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_none_or(|name| !known.contains(&name))
    }) {
        return Err(LegacyImportError::DestinationNotNew);
    }

    match fs::symlink_metadata(root.join("objects")) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let objects = SafeDir::open(root.join("objects"))?;
            let mut existing =
                fs::read_dir(root.join("objects"))?.collect::<Result<Vec<_>, _>>()?;
            existing.sort_by_key(fs::DirEntry::file_name);
            for entry in existing {
                let name = entry
                    .file_name()
                    .to_str()
                    .ok_or(LegacyImportError::DestinationNotNew)?
                    .to_owned();
                if !source_objects.iter().any(|source| source == &name) {
                    return Err(LegacyImportError::DestinationNotNew);
                }
                objects.read_blob(&name)?;
            }
        }
        Ok(_) => return Err(LegacyImportError::DestinationNotNew),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn object_names(root: &Path) -> Result<Vec<String>, LegacyImportError> {
    let path = root.join("objects");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(LegacyImportError::SourceChanged);
    }
    let directory = SafeDir::open(&path)?;
    let mut entries = fs::read_dir(&path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut names = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry
            .file_name()
            .to_str()
            .ok_or(LegacyImportError::SourceChanged)?
            .to_owned();
        directory.read_blob(&name)?;
        names.push(name);
    }
    Ok(names)
}

fn validate_source_snapshot(
    transaction: &Transaction<'_>,
    source_root: &Path,
    inventory: &LegacyStateInventory,
) -> Result<(), LegacyImportError> {
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let schema_valid = match inventory.schema {
        LegacySchema::GoV1 => crate::validate_v1_tables(transaction, false).is_ok(),
        LegacySchema::RoverRustV1 => crate::validate_v1(transaction).is_ok(),
        LegacySchema::EmptyUnversioned => {
            version == 0 && matches!(crate::has_user_schema_objects(transaction), Ok(false))
        }
        LegacySchema::Incompatible => false,
    };
    if !schema_valid {
        return Err(LegacyImportError::SourceChanged);
    }
    let (records, _, _) = if version == 0 {
        (super::TableInventory::default(), 0, 0)
    } else {
        inspect_records(transaction).map_err(LegacyImportError::Sqlite)?
    };
    let events = if version == 0 {
        super::TableInventory::default()
    } else {
        inspect_events(transaction).map_err(LegacyImportError::Sqlite)?
    };
    let request_keys = if version == 0 {
        super::TableInventory::default()
    } else {
        inspect_request_keys(transaction).map_err(LegacyImportError::Sqlite)?
    };
    let (objects, blockers) = inspect_objects(source_root)?;
    if !blockers.is_empty() {
        return Err(LegacyImportError::SourceChanged);
    }
    let source_digest = combined_digest(&[
        ("records", &records.digest),
        ("events", &events.digest),
        ("request_keys", &request_keys.digest),
        ("objects", &objects.digest),
    ]);
    if source_digest != inventory.dry_run.source_digest {
        return Err(LegacyImportError::SourceChanged);
    }
    Ok(())
}

fn copy_objects(
    source_root: &Path,
    destination: &StateRoot,
    inventory: &LegacyStateInventory,
) -> Result<u64, LegacyImportError> {
    let names = object_names(source_root)?;
    if names.is_empty() {
        if inventory.objects.verified != 0 {
            return Err(LegacyImportError::SourceChanged);
        }
        return Ok(0);
    }
    let source = SafeDir::open(source_root.join("objects"))?;
    let mut hash = Sha256::new();
    let mut copied = 0_u64;
    for name in names {
        let bytes = source.read_blob(&name)?;
        let digest = destination.put_blob(&bytes)?.to_hex();
        if digest != name {
            return Err(LegacyImportError::SourceChanged);
        }
        update_field(&mut hash, name.as_bytes());
        update_field(
            &mut hash,
            &u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes(),
        );
        copied += 1;
    }
    let digest = hex_digest(hash.finalize().as_slice());
    if digest != inventory.objects.digest {
        return Err(LegacyImportError::SourceChanged);
    }
    Ok(copied)
}

fn ensure_destination_empty(transaction: &Transaction<'_>) -> Result<(), LegacyImportError> {
    let (records, events, request_keys): (i64, i64, i64) = transaction.query_row(
        "SELECT (SELECT count(*) FROM records),(SELECT count(*) FROM events),(SELECT count(*) FROM request_keys)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if records != 0 || events != 0 || request_keys != 0 {
        return Err(LegacyImportError::DestinationNotNew);
    }
    Ok(())
}

fn copy_records(
    source: &Transaction<'_>,
    destination: &Transaction<'_>,
) -> Result<RecordImportCounts, LegacyImportError> {
    let mut statement =
        source.prepare("SELECT kind,id,payload,updated FROM records ORDER BY kind,id")?;
    let mut rows = statement.query([])?;
    let mut counts = RecordImportCounts::default();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let id: String = row.get(1)?;
        let payload: String = row.get(2)?;
        let updated: String = row.get(3)?;
        if kind == "lease" {
            validate_lease(&id, &payload)?;
            let released = serde_json::json!({"deleted": true, "reason": "migration"});
            let released = crate::encode_payload(&released)?;
            insert_event_payload(destination, "lease.migration_released", &id, &released)?;
            counts.leases_released += 1;
            counts.sanitized_records += 1;
            continue;
        }
        let (imported_payload, sanitized, revoked, marked_lost) =
            sanitize_record(&kind, &id, &payload)?;
        let imported_updated = if sanitized {
            Timestamp::now()
                .to_rfc3339()
                .map_err(|error| LegacyImportError::Timestamp(error.to_string()))?
        } else {
            updated
        };
        destination.execute(
            "INSERT INTO records(kind,id,payload,updated) VALUES(?1,?2,?3,?4)",
            (&kind, &id, &imported_payload, &imported_updated),
        )?;
        if sanitized {
            insert_event_payload(destination, SANITIZE_EVENT, &id, &imported_payload)?;
            counts.sanitized_records += 1;
        }
        counts.records += 1;
        counts.grants_revoked += u64::from(revoked);
        counts.executions_marked_lost += u64::from(marked_lost);
    }
    Ok(counts)
}

fn validate_lease(id: &str, payload: &str) -> Result<(), LegacyImportError> {
    let value = crate::parse_payload(payload)?;
    let object = value
        .as_value()
        .as_object()
        .ok_or_else(|| LegacyImportError::InvalidRecord("lease is not an object".to_owned()))?;
    let owner = object
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyImportError::InvalidRecord("lease owner is missing".to_owned()))?;
    let resource = object
        .get("resource")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyImportError::InvalidRecord("lease resource is missing".to_owned()))?;
    if Id::parse(owner).is_err() || resource != id {
        return Err(LegacyImportError::InvalidRecord(
            "lease owner or resource key does not match".to_owned(),
        ));
    }
    Ok(())
}

fn sanitize_record(
    kind: &str,
    id: &str,
    payload: &str,
) -> Result<(String, bool, bool, bool), LegacyImportError> {
    if !matches!(kind, "grant" | "task" | "workflow") {
        return Ok((payload.to_owned(), false, false, false));
    }
    let mut value = crate::parse_payload(payload)?;
    let object = value
        .as_value_mut()
        .as_object_mut()
        .ok_or_else(|| LegacyImportError::InvalidRecord(format!("{kind} {id} is not an object")))?;
    let embedded_id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyImportError::InvalidRecord(format!("{kind} {id} has no string ID")))?;
    if embedded_id != id {
        return Err(LegacyImportError::InvalidRecord(format!(
            "{kind} {id} has mismatched embedded ID {embedded_id}"
        )));
    }

    let mut revoked = false;
    let mut marked_lost = false;
    if kind == "grant" {
        object.insert("revoked".to_owned(), Value::Bool(true));
        revoked = true;
    } else {
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !is_terminal_status(status) && status != "CONFLICT" {
            object.insert("status".to_owned(), Value::String("LOST".to_owned()));
            object.insert("error".to_owned(), Value::String(SANITIZE_ERROR.to_owned()));
            marked_lost = true;
        }
        object.insert("pid".to_owned(), Value::Number(0.into()));
        object.remove("socket");
        object.remove("process_identity");
    }
    let encoded = crate::encode_payload(value.as_value())?;
    Ok((encoded, true, revoked, marked_lost))
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "CANDIDATE_READY"
            | "REVIEW_READY"
            | "CHECKS_BLOCKED"
            | "FAILED"
            | "ERROR"
            | "CANCELLED"
            | "TIMED_OUT"
            | "LOST"
    )
}

fn copy_events(
    source: &Transaction<'_>,
    destination: &Transaction<'_>,
) -> Result<(), LegacyImportError> {
    let mut statement =
        source.prepare("SELECT seq,kind,ref,payload,at FROM events ORDER BY seq")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let sequence: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let reference: String = row.get(2)?;
        let payload: String = row.get(3)?;
        let at: String = row.get(4)?;
        destination.execute(
            "INSERT INTO events(seq,kind,ref,payload,at) VALUES(?1,?2,?3,?4,?5)",
            (sequence, kind, reference, payload, at),
        )?;
    }
    Ok(())
}

fn copy_request_keys(
    source: &Transaction<'_>,
    destination: &Transaction<'_>,
) -> Result<(), LegacyImportError> {
    let mut statement = source.prepare("SELECT key,digest,ref FROM request_keys ORDER BY key")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let digest: String = row.get(1)?;
        let reference: String = row.get(2)?;
        destination.execute(
            "INSERT INTO request_keys(key,digest,ref) VALUES(?1,?2,?3)",
            (key, digest, reference),
        )?;
    }
    Ok(())
}

fn verify_receipt_destination(
    source: &Transaction<'_>,
    connection: &Connection,
    root: &Path,
    receipt: &LegacyImportReceipt,
    source_objects: &[String],
    inventory: &LegacyStateInventory,
) -> Result<(), LegacyImportError> {
    if receipt.events != inventory.events.rows
        || receipt.request_keys != inventory.request_keys.rows
        || receipt.objects != inventory.objects.verified
    {
        return Err(LegacyImportError::InvalidReceipt);
    }
    let generated_events = verify_receipt_records(source, connection, receipt, inventory)?;
    verify_receipt_events(source, connection, receipt, inventory, generated_events)?;
    verify_receipt_request_keys(source, connection, receipt, inventory)?;
    verify_receipt_record_count(connection, receipt)?;
    verify_receipt_object_set(root, receipt, source_objects)
}

fn verify_receipt_records(
    source: &Transaction<'_>,
    destination: &Connection,
    receipt: &LegacyImportReceipt,
    inventory: &LegacyStateInventory,
) -> Result<Vec<(String, String, String)>, LegacyImportError> {
    let mut expected_events = Vec::new();
    let mut counts = RecordImportCounts::default();
    if inventory.user_version != 0 {
        let mut statement =
            source.prepare("SELECT kind,id,payload,updated FROM records ORDER BY kind,id")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let kind: String = row.get(0)?;
            let id: String = row.get(1)?;
            let payload: String = row.get(2)?;
            let updated: String = row.get(3)?;
            if kind == "lease" {
                validate_lease(&id, &payload)?;
                let event_payload = crate::encode_payload(
                    &serde_json::json!({"deleted": true, "reason": "migration"}),
                )?;
                expected_events.push(("lease.migration_released".to_owned(), id, event_payload));
                counts.leases_released += 1;
                counts.sanitized_records += 1;
                continue;
            }
            let (expected_payload, sanitized, revoked, marked_lost) =
                sanitize_record(&kind, &id, &payload)?;
            let actual: Option<(String, String)> = destination
                .query_row(
                    "SELECT payload,updated FROM records WHERE kind=?1 AND id=?2",
                    (&kind, &id),
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((actual_payload, actual_updated)) = actual else {
                return Err(LegacyImportError::InvalidReceipt);
            };
            if actual_payload != expected_payload
                || (!sanitized && actual_updated != updated)
                || (sanitized && actual_updated.is_empty())
            {
                return Err(LegacyImportError::InvalidReceipt);
            }
            if sanitized {
                expected_events.push((SANITIZE_EVENT.to_owned(), id, expected_payload));
                counts.sanitized_records += 1;
            }
            counts.records += 1;
            counts.grants_revoked += u64::from(revoked);
            counts.executions_marked_lost += u64::from(marked_lost);
        }
    }
    if counts.records != receipt.records
        || counts.grants_revoked != receipt.grants_revoked
        || counts.leases_released != receipt.leases_released
        || counts.executions_marked_lost != receipt.executions_marked_lost
        || counts.sanitized_records != receipt.sanitized_records
    {
        return Err(LegacyImportError::InvalidReceipt);
    }
    Ok(expected_events)
}

fn verify_receipt_events(
    source: &Transaction<'_>,
    destination: &Connection,
    receipt: &LegacyImportReceipt,
    inventory: &LegacyStateInventory,
    generated_events: Vec<(String, String, String)>,
) -> Result<(), LegacyImportError> {
    let mut expected_source = Vec::new();
    if inventory.user_version != 0 {
        let mut statement =
            source.prepare("SELECT seq,kind,ref,payload,at FROM events ORDER BY seq")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            expected_source.push((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ));
        }
    }
    let mut actual = Vec::new();
    let mut statement =
        destination.prepare("SELECT seq,kind,ref,payload,at FROM events ORDER BY seq")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        actual.push((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ));
    }
    let expected_tail_length = generated_events
        .len()
        .checked_add(1)
        .ok_or(LegacyImportError::InvalidReceipt)?;
    let expected_event_count = expected_source
        .len()
        .checked_add(expected_tail_length)
        .ok_or(LegacyImportError::InvalidReceipt)?;
    if actual.len() != expected_event_count
        || actual.get(..expected_source.len()) != Some(expected_source.as_slice())
    {
        return Err(LegacyImportError::InvalidReceipt);
    }
    let mut expected_tail = generated_events;
    expected_tail.push((
        IMPORT_EVENT.to_owned(),
        receipt.source_root_digest.clone(),
        crate::encode_payload(receipt)?,
    ));
    let actual_tail = actual
        .iter()
        .skip(expected_source.len())
        .map(|(_, kind, reference, payload, at)| {
            if at.is_empty() {
                Err(LegacyImportError::InvalidReceipt)
            } else {
                Ok((kind.clone(), reference.clone(), payload.clone()))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if actual_tail != expected_tail {
        return Err(LegacyImportError::InvalidReceipt);
    }
    Ok(())
}

fn verify_receipt_request_keys(
    source: &Transaction<'_>,
    destination: &Connection,
    receipt: &LegacyImportReceipt,
    inventory: &LegacyStateInventory,
) -> Result<(), LegacyImportError> {
    let mut expected = Vec::new();
    if inventory.user_version != 0 {
        let mut statement =
            source.prepare("SELECT key,digest,ref FROM request_keys ORDER BY key")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            expected.push((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ));
        }
    }
    let mut actual = Vec::new();
    let mut statement =
        destination.prepare("SELECT key,digest,ref FROM request_keys ORDER BY key")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        actual.push((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ));
    }
    if actual != expected || u64::try_from(actual.len()).unwrap_or(u64::MAX) != receipt.request_keys
    {
        return Err(LegacyImportError::InvalidReceipt);
    }
    Ok(())
}

fn verify_receipt_record_count(
    connection: &Connection,
    receipt: &LegacyImportReceipt,
) -> Result<(), LegacyImportError> {
    let (actual_records, actual_events, actual_keys): (i64, i64, i64) = connection.query_row(
        "SELECT (SELECT count(*) FROM records),(SELECT count(*) FROM events),(SELECT count(*) FROM request_keys)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let expected_records = receipt
        .records
        .checked_add(1)
        .and_then(|count| i64::try_from(count).ok())
        .ok_or(LegacyImportError::InvalidReceipt)?;
    let expected_events = receipt
        .events
        .checked_add(receipt.sanitized_records)
        .and_then(|count| count.checked_add(1))
        .and_then(|count| i64::try_from(count).ok())
        .ok_or(LegacyImportError::InvalidReceipt)?;
    let expected_keys =
        i64::try_from(receipt.request_keys).map_err(|_| LegacyImportError::InvalidReceipt)?;
    if actual_records != expected_records
        || actual_events != expected_events
        || actual_keys != expected_keys
    {
        return Err(LegacyImportError::InvalidReceipt);
    }
    let expected_receipt = crate::encode_payload(receipt)?;
    let actual_receipt: String = connection.query_row(
        "SELECT payload FROM records WHERE kind=?1 AND id=?2",
        (IMPORT_KIND, &receipt.source_root_digest),
        |row| row.get(0),
    )?;
    if actual_receipt != expected_receipt {
        return Err(LegacyImportError::InvalidReceipt);
    }
    Ok(())
}

fn verify_receipt_object_set(
    root: &Path,
    receipt: &LegacyImportReceipt,
    source_objects: &[String],
) -> Result<(), LegacyImportError> {
    let objects_path = root.join("objects");
    let object_count = match fs::symlink_metadata(&objects_path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let objects = SafeDir::open(&objects_path)?;
            let entries = fs::read_dir(&objects_path)?.collect::<Result<Vec<_>, _>>()?;
            let mut names = Vec::with_capacity(entries.len());
            for entry in &entries {
                let filename = entry.file_name();
                let name = filename.to_str().ok_or(LegacyImportError::InvalidReceipt)?;
                names.push(name.to_owned());
                objects
                    .read_blob(name)
                    .map_err(|_| LegacyImportError::InvalidReceipt)?;
            }
            names.sort();
            if names != source_objects {
                return Err(LegacyImportError::InvalidReceipt);
            }
            u64::try_from(entries.len()).map_err(|_| LegacyImportError::InvalidReceipt)?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        _ => return Err(LegacyImportError::InvalidReceipt),
    };
    if object_count != receipt.objects {
        return Err(LegacyImportError::InvalidReceipt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::{LegacyImportError, LegacyImportOptions, LegacyStateInventory};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rover-legacy-import-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create parent");
            set_private(&path);
            Self(path)
        }

        fn source(&self) -> PathBuf {
            let root = self.0.join("source");
            fs::create_dir(&root).expect("create source root");
            set_private(&root);
            let database = Connection::open(root.join("rover.db")).expect("create database");
            database
                .execute_batch(
                    "CREATE TABLE records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id));\
                     CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL);\
                     CREATE TABLE request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL);\
                     PRAGMA user_version=1;\
                     INSERT INTO records VALUES('thing','thing1','{\"name\":\"kept\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO records VALUES('grant','grant1','{\"id\":\"grant1\",\"revoked\":false}','2026-01-01T00:00:00Z');\
                     INSERT INTO records VALUES('task','task1','{\"id\":\"task1\",\"status\":\"RUNNING\",\"pid\":55,\"socket\":\"/tmp/worker.sock\",\"process_identity\":\"worker-identity\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO records VALUES('workflow','flow1','{\"id\":\"flow1\",\"status\":\"RUNNING\",\"pid\":56,\"socket\":\"/tmp/flow.sock\",\"process_identity\":\"flow-identity\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO records VALUES('lease','resource1','{\"owner\":\"agent1\",\"resource\":\"resource1\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO events(kind,ref,payload,at) VALUES('thing.created','thing1','{\"name\":\"kept\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO request_keys VALUES('request1','request-digest','task1');",
                )
                .expect("create Go v1 data");
            drop(database);
            let objects = root.join("objects");
            fs::create_dir(&objects).expect("create object directory");
            set_private(&objects);
            let object = b"retained migration object";
            let digest = rover_core::Sha256Digest::of(object).to_hex();
            fs::write(objects.join(digest), object).expect("write content addressed object");
            root
        }

        fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
            fn visit(root: &Path, prefix: &str, output: &mut Vec<(String, Vec<u8>)>) {
                let mut entries = fs::read_dir(root)
                    .expect("read directory")
                    .map(|entry| entry.expect("directory entry"))
                    .collect::<Vec<_>>();
                entries.sort_by_key(fs::DirEntry::file_name);
                for entry in entries {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let relative = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    };
                    let metadata = fs::symlink_metadata(entry.path()).expect("stat entry");
                    if metadata.is_dir() {
                        output.push((relative.clone(), b"directory".to_vec()));
                        visit(&entry.path(), &relative, output);
                    } else {
                        output.push((relative, fs::read(entry.path()).expect("read file")));
                    }
                }
            }
            let mut output = Vec::new();
            visit(root, "", &mut output);
            output
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn set_private(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("set private permissions");
        }
    }

    fn options() -> LegacyImportOptions {
        LegacyImportOptions {
            source_quiesced: true,
            accept_excluded_runtime_data: true,
        }
    }

    #[test]
    fn import_copies_snapshot_sanitizes_runtime_state_and_is_idempotent() {
        let parent = TestRoot::new();
        let source = parent.source();
        let destination = parent.0.join("destination");
        let before = TestRoot::snapshot(&source);

        let first = LegacyStateInventory::import_to(&source, &destination, options())
            .expect("import Go state");
        assert!(!first.already_imported);
        assert_eq!(first.receipt.records, 4);
        assert_eq!(first.receipt.events, 1);
        assert_eq!(first.receipt.request_keys, 1);
        assert_eq!(first.receipt.objects, 1);
        assert_eq!(first.receipt.grants_revoked, 1);
        assert_eq!(first.receipt.leases_released, 1);
        assert_eq!(first.receipt.executions_marked_lost, 2);
        assert_eq!(first.receipt.sanitized_records, 4);
        assert_eq!(
            TestRoot::snapshot(&source),
            before,
            "source tree was modified"
        );

        let connection = Connection::open(destination.join("rover.db")).expect("open result");
        let grant: String = connection
            .query_row(
                "SELECT payload FROM records WHERE kind='grant'",
                [],
                |row| row.get(0),
            )
            .expect("read grant");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&grant).unwrap()["revoked"],
            true
        );
        for (kind, id) in [("task", "task1"), ("workflow", "flow1")] {
            let payload: String = connection
                .query_row(
                    "SELECT payload FROM records WHERE kind=?1 AND id=?2",
                    (kind, id),
                    |row| row.get(0),
                )
                .expect("read sanitized execution");
            let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(value["status"], "LOST");
            assert_eq!(value["pid"], 0);
            assert!(value.get("socket").is_none());
            assert!(value.get("process_identity").is_none());
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM records WHERE kind='lease'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM events WHERE kind='thing.created'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM request_keys WHERE key='request1'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        drop(connection);
        let object_name = fs::read_dir(source.join("objects"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        assert_eq!(
            fs::read(destination.join("objects").join(object_name)).unwrap(),
            b"retained migration object"
        );

        let repeated = LegacyStateInventory::import_to(&source, &destination, options())
            .expect("repeat import");
        assert!(repeated.already_imported);
        assert_eq!(repeated.receipt, first.receipt);
        assert_eq!(
            TestRoot::snapshot(&source),
            before,
            "repeat modified source"
        );
    }

    #[test]
    fn import_requires_quiescence_and_runtime_exclusion_acknowledgement() {
        let parent = TestRoot::new();
        let source = parent.source();
        let destination = parent.0.join("destination");
        let not_quiesced = LegacyImportOptions {
            source_quiesced: false,
            accept_excluded_runtime_data: true,
        };
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, not_quiesced),
            Err(LegacyImportError::SourceNotQuiesced)
        ));

        fs::write(source.join("operator-note.txt"), b"retained in source").unwrap();
        let no_exclusion_ack = LegacyImportOptions {
            source_quiesced: true,
            accept_excluded_runtime_data: false,
        };
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, no_exclusion_ack),
            Err(LegacyImportError::ExcludedData(_))
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn repeat_import_rejects_same_count_record_changes_and_extra_objects() {
        let parent = TestRoot::new();
        let source = parent.source();
        let destination = parent.0.join("destination");
        LegacyStateInventory::import_to(&source, &destination, options()).expect("initial import");

        let connection = Connection::open(destination.join("rover.db")).expect("open result");
        connection
            .execute(
                "UPDATE records SET payload=?1 WHERE kind='thing' AND id='thing1'",
                ["{\"name\":\"tampered\"}"],
            )
            .expect("tamper with imported payload without changing row count");
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, options()),
            Err(LegacyImportError::InvalidReceipt)
        ));
        connection
            .execute(
                "UPDATE records SET payload=?1 WHERE kind='thing' AND id='thing1'",
                ["{\"name\":\"kept\"}"],
            )
            .expect("restore imported payload");
        connection
            .execute(
                "UPDATE events SET payload='{}' WHERE kind='thing.created'",
                [],
            )
            .expect("tamper with copied source event");
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, options()),
            Err(LegacyImportError::InvalidReceipt)
        ));
        connection
            .execute(
                "UPDATE events SET payload=?1 WHERE kind='thing.created'",
                ["{\"name\":\"kept\"}"],
            )
            .expect("restore copied event");
        connection
            .execute(
                "UPDATE request_keys SET digest='changed' WHERE key='request1'",
                [],
            )
            .expect("tamper with copied request key");
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, options()),
            Err(LegacyImportError::InvalidReceipt)
        ));
        connection
            .execute(
                "UPDATE request_keys SET digest='request-digest' WHERE key='request1'",
                [],
            )
            .expect("restore copied request key");
        drop(connection);

        crate::files::StateRoot::open(&destination)
            .expect("open imported state root")
            .put_blob(b"unexpected but valid content-addressed object")
            .expect("add an independently valid object");
        assert!(matches!(
            LegacyStateInventory::import_to(&source, &destination, options()),
            Err(LegacyImportError::InvalidReceipt)
        ));
    }
}
