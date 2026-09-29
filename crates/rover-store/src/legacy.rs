//! Read-only inspection and migration dry-run reporting for Go Rover state.
//!
//! The inspector never initializes, migrates, checkpoints, or writes to the
//! source. It reports database compatibility, row counts/digests, invalid JSON
//! and identifiers, and whether content-addressed objects verify. Live task and
//! check trees are called out but are not included in the import scope.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rover_core::Id;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::files::{canonical_state_path, SafeDir};

mod import;
pub use import::{LegacyImportError, LegacyImportOptions, LegacyImportReceipt, LegacyImportReport};

/// The inspected database's migration compatibility class.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySchema {
    /// The Go v1 schema, which has no Rust migration ledger.
    GoV1,
    /// A Rust v1 database that already has the reviewed migration ledger.
    RoverRustV1,
    /// An empty `SQLite` file that Rust could initialize as a new state store.
    EmptyUnversioned,
    /// A schema that cannot safely be imported by the current Rust version.
    Incompatible,
}

/// Counts and a stable digest for one imported SQL table.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TableInventory {
    /// Number of rows in the table.
    pub rows: u64,
    /// Total UTF-8 byte count of the row payload fields reported by the query.
    pub payload_bytes: u64,
    /// SHA-256 over the ordered, length-framed source fields.
    pub digest: String,
}

/// Counts for verified content-addressed objects in the Go `objects/` folder.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectInventory {
    /// Number of digest-named object files verified successfully.
    pub verified: u64,
    /// Total bytes in verified objects.
    pub bytes: u64,
    /// SHA-256 over the ordered object digest names and verified sizes.
    pub digest: String,
}

/// Metadata-only inventory of the live task/check filesystem tree. File bytes
/// are not read, hashed, or included in the database/object import scope.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeTreeInventory {
    /// Regular files found without following symlinks.
    pub files: u64,
    /// Child directories found (the tree root itself is not counted).
    pub directories: u64,
    /// Bytes in regular files.
    pub bytes: u64,
    /// Symlinks observed and deliberately not traversed.
    pub symlinks: u64,
    /// Sockets, devices, and other non-regular entries observed.
    pub other_entries: u64,
}

/// Dry-run outcome. It gives prospective import counts and blockers without
/// creating a destination or touching source data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationDryRun {
    /// Whether database rows and verified objects can be imported after the
    /// source has been quiesced.
    pub database_and_objects_importable_after_quiesce: bool,
    /// Records expected to import.
    pub records: u64,
    /// Events expected to import.
    pub events: u64,
    /// Idempotency keys expected to import.
    pub request_keys: u64,
    /// Content-addressed objects expected to import.
    pub objects: u64,
    /// Count of invalid record identifiers.
    pub invalid_record_ids: u64,
    /// Count of malformed record JSON payloads.
    pub malformed_record_json: u64,
    /// Reasons this state cannot be imported as reported.
    pub blockers: Vec<String>,
    /// Important source data outside the database/object import scope.
    pub warnings: Vec<String>,
    /// Combined digest of database tables and verified objects.
    pub source_digest: String,
}

/// Read-only inventory of an existing Go or Rover Rust state root.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyStateInventory {
    /// Canonical absolute source directory.
    pub root: PathBuf,
    /// `SQLite` `user_version`.
    pub user_version: i64,
    /// User table names discovered in `sqlite_master`.
    pub tables: Vec<String>,
    /// Compatibility assessment for the database schema.
    pub schema: LegacySchema,
    /// Size of the main `SQLite` database file. WAL sidecars are not included.
    pub database_bytes: u64,
    /// Record table counts and digest.
    pub records: TableInventory,
    /// Event table counts and digest.
    pub events: TableInventory,
    /// Request-key table counts and digest.
    pub request_keys: TableInventory,
    /// Content-addressed object inventory.
    pub objects: ObjectInventory,
    /// Metadata-only inventory of task workspaces and task outputs.
    pub task_tree: RuntimeTreeInventory,
    /// Metadata-only inventory of check workspaces and outputs.
    pub check_tree: RuntimeTreeInventory,
    /// Unrecognized top-level entries left out of the database/object import.
    pub unmanaged_root_entries: Vec<String>,
    /// Prospective migration assessment.
    pub dry_run: MigrationDryRun,
}

impl LegacyStateInventory {
    /// Inspect a Go/Rover state root without writing to it.
    ///
    /// Database access is `SQLite` read-only plus `query_only`; no migration,
    /// journal checkpoint, or filesystem creation is performed. A normal
    /// `SQLite` read transaction provides a consistent view of the three data
    /// tables. Files in `tasks/` and `checks/` may change independently, so the
    /// caller must stop Rover before relying on the dry-run as a migration
    /// approval.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unsafe state paths, unreadable files, or
    /// `SQLite` inspection failures.
    #[allow(clippy::too_many_lines)]
    pub fn inspect(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = canonical_state_path(root.as_ref())?;
        let _root_handle = SafeDir::open(&root)?;
        let database_path = root.join("rover.db");
        let database_metadata = fs::symlink_metadata(&database_path)?;
        if !database_metadata.file_type().is_file() {
            return Err(invalid_data("state database must be a regular file"));
        }

        let mut connection = Connection::open_with_flags(
            &database_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )
        .map_err(sqlite_io)?;
        connection
            .execute_batch("PRAGMA query_only=ON")
            .map_err(sqlite_io)?;
        let transaction = connection.transaction().map_err(sqlite_io)?;

        let user_version: i64 = transaction
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(sqlite_io)?;
        let mut tables = transaction
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' \
                 AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(sqlite_io)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_io)?;
        tables.sort();

        let schema = match user_version {
            1 => {
                let ledger = tables.iter().any(|table| table == "schema_migrations");
                if ledger {
                    if crate::validate_v1(&transaction).is_ok() {
                        LegacySchema::RoverRustV1
                    } else {
                        LegacySchema::Incompatible
                    }
                } else if crate::validate_v1_tables(&transaction, false).is_ok() {
                    LegacySchema::GoV1
                } else {
                    LegacySchema::Incompatible
                }
            }
            0 if !crate::has_user_schema_objects(&transaction).map_err(migration_io)? => {
                LegacySchema::EmptyUnversioned
            }
            _ => LegacySchema::Incompatible,
        };

        let mut blockers = Vec::new();
        if schema == LegacySchema::Incompatible {
            blockers.push(format!(
                "database schema version {user_version} or its table definitions are not compatible"
            ));
        }

        let mut records = TableInventory::default();
        let mut events = TableInventory::default();
        let mut request_keys = TableInventory::default();
        let mut invalid_record_ids = 0;
        let mut malformed_record_json = 0;
        if matches!(schema, LegacySchema::GoV1 | LegacySchema::RoverRustV1) {
            (records, invalid_record_ids, malformed_record_json) =
                inspect_records(&transaction).map_err(sqlite_io)?;
            events = inspect_events(&transaction).map_err(sqlite_io)?;
            request_keys = inspect_request_keys(&transaction).map_err(sqlite_io)?;
            if invalid_record_ids > 0 {
                blockers.push(format!(
                    "{invalid_record_ids} records have invalid kind or ID values"
                ));
            }
            if malformed_record_json > 0 {
                blockers.push(format!(
                    "{malformed_record_json} records have malformed JSON payloads"
                ));
            }
        }

        transaction.commit().map_err(sqlite_io)?;

        let (objects, object_blockers) = inspect_objects(&root)?;
        blockers.extend(object_blockers);
        let mut warnings = Vec::new();
        let task_tree = inspect_runtime_tree(&root.join("tasks"))?;
        let check_tree = inspect_runtime_tree(&root.join("checks"))?;
        if tree_has_content(&task_tree) {
            warnings.push(format!(
                "tasks/ contains {} files ({} bytes), {} directories, {} symlinks, and {} other entries; it is inventoried but excluded from this import",
                task_tree.files,
                task_tree.bytes,
                task_tree.directories,
                task_tree.symlinks,
                task_tree.other_entries
            ));
        }
        if tree_has_content(&check_tree) {
            warnings.push(format!(
                "checks/ contains {} files ({} bytes), {} directories, {} symlinks, and {} other entries; it is inventoried but excluded from this import",
                check_tree.files,
                check_tree.bytes,
                check_tree.directories,
                check_tree.symlinks,
                check_tree.other_entries
            ));
        }
        let unmanaged_root_entries = inspect_unmanaged_root_entries(&root)?;
        if !unmanaged_root_entries.is_empty() {
            warnings.push(format!(
                "unmanaged root entries are preserved and excluded: {}",
                unmanaged_root_entries.join(", ")
            ));
        }
        warnings.push(
            "the source must be quiesced before import; SQLite rows and filesystem objects do not share one atomic snapshot".to_owned(),
        );

        let objects_digest = objects.digest.clone();
        let source_digest = combined_digest(&[
            ("records", &records.digest),
            ("events", &events.digest),
            ("request_keys", &request_keys.digest),
            ("objects", &objects_digest),
        ]);
        let database_and_objects_importable_after_quiesce = blockers.is_empty()
            && matches!(
                schema,
                LegacySchema::GoV1 | LegacySchema::RoverRustV1 | LegacySchema::EmptyUnversioned
            );
        let dry_run = MigrationDryRun {
            database_and_objects_importable_after_quiesce,
            records: records.rows,
            events: events.rows,
            request_keys: request_keys.rows,
            objects: objects.verified,
            invalid_record_ids,
            malformed_record_json,
            blockers,
            warnings,
            source_digest,
        };

        Ok(Self {
            root,
            user_version,
            tables,
            schema,
            database_bytes: database_metadata.len(),
            records,
            events,
            request_keys,
            objects,
            task_tree,
            check_tree,
            unmanaged_root_entries,
            dry_run,
        })
    }
}

fn inspect_records(connection: &Connection) -> rusqlite::Result<(TableInventory, u64, u64)> {
    let mut statement =
        connection.prepare("SELECT kind,id,payload,updated FROM records ORDER BY kind,id")?;
    let mut rows = statement.query([])?;
    let mut inventory = TableInventory::default();
    let mut digest = Sha256::new();
    let mut invalid_ids = 0;
    let mut malformed_json = 0;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let id: String = row.get(1)?;
        let payload: String = row.get(2)?;
        let updated: String = row.get(3)?;
        update_field(&mut digest, kind.as_bytes());
        update_field(&mut digest, id.as_bytes());
        update_field(&mut digest, payload.as_bytes());
        update_field(&mut digest, updated.as_bytes());
        if Id::parse(kind.as_str()).is_err() || Id::parse(id.as_str()).is_err() {
            invalid_ids += 1;
        }
        if crate::parse_payload(&payload).is_err() {
            malformed_json += 1;
        }
        inventory.payload_bytes += u64::try_from(payload.len()).unwrap_or(u64::MAX);
        inventory.rows += 1;
    }
    inventory.digest = hex_digest(digest.finalize().as_slice());
    Ok((inventory, invalid_ids, malformed_json))
}

fn inspect_events(connection: &Connection) -> rusqlite::Result<TableInventory> {
    let mut statement =
        connection.prepare("SELECT seq,kind,ref,payload,at FROM events ORDER BY seq")?;
    let mut rows = statement.query([])?;
    let mut inventory = TableInventory::default();
    let mut digest = Sha256::new();
    while let Some(row) = rows.next()? {
        let sequence: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let reference: String = row.get(2)?;
        let payload: String = row.get(3)?;
        let at: String = row.get(4)?;
        update_field(&mut digest, &sequence.to_be_bytes());
        update_field(&mut digest, kind.as_bytes());
        update_field(&mut digest, reference.as_bytes());
        update_field(&mut digest, payload.as_bytes());
        update_field(&mut digest, at.as_bytes());
        inventory.payload_bytes += u64::try_from(payload.len()).unwrap_or(u64::MAX);
        inventory.rows += 1;
    }
    inventory.digest = hex_digest(digest.finalize().as_slice());
    Ok(inventory)
}

fn inspect_request_keys(connection: &Connection) -> rusqlite::Result<TableInventory> {
    let mut statement =
        connection.prepare("SELECT key,digest,ref FROM request_keys ORDER BY key")?;
    let mut rows = statement.query([])?;
    let mut inventory = TableInventory::default();
    let mut digest = Sha256::new();
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let value_digest: String = row.get(1)?;
        let reference: String = row.get(2)?;
        update_field(&mut digest, key.as_bytes());
        update_field(&mut digest, value_digest.as_bytes());
        update_field(&mut digest, reference.as_bytes());
        inventory.payload_bytes +=
            u64::try_from(key.len() + value_digest.len() + reference.len()).unwrap_or(u64::MAX);
        inventory.rows += 1;
    }
    inventory.digest = hex_digest(digest.finalize().as_slice());
    Ok(inventory)
}

fn inspect_objects(root: &Path) -> io::Result<(ObjectInventory, Vec<String>)> {
    let path = root.join("objects");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((ObjectInventory::default(), Vec::new()));
        }
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok((
            ObjectInventory::default(),
            vec!["objects/ is not a regular directory".to_owned()],
        ));
    }
    let directory = match SafeDir::open(&path) {
        Ok(directory) => directory,
        Err(error) => {
            return Ok((
                ObjectInventory::default(),
                vec![format!("objects/ cannot be safely opened: {error}")],
            ));
        }
    };
    let mut entries = fs::read_dir(&path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut inventory = ObjectInventory::default();
    let mut digest = Sha256::new();
    let mut blockers = Vec::new();
    for entry in entries {
        let name = if let Some(name) = entry.file_name().to_str() {
            name.to_owned()
        } else {
            blockers.push("objects/ contains a non-UTF-8 filename".to_owned());
            continue;
        };
        let entry_metadata = fs::symlink_metadata(entry.path())?;
        if entry_metadata.file_type().is_symlink() || !entry_metadata.is_file() {
            blockers.push(format!("objects/{name} is not a regular object file"));
            continue;
        }
        match directory.read_blob(&name) {
            Ok(bytes) => {
                update_field(&mut digest, name.as_bytes());
                update_field(
                    &mut digest,
                    &u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes(),
                );
                inventory.bytes += u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                inventory.verified += 1;
            }
            Err(error) => blockers.push(format!(
                "objects/{name} failed digest/size validation: {error}"
            )),
        }
    }
    inventory.digest = hex_digest(digest.finalize().as_slice());
    Ok((inventory, blockers))
}

fn inspect_runtime_tree(root: &Path) -> io::Result<RuntimeTreeInventory> {
    fn visit(path: &Path, inventory: &mut RuntimeTreeInventory, is_root: bool) -> io::Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() {
            inventory.symlinks += 1;
        } else if metadata.is_dir() {
            if !is_root {
                inventory.directories += 1;
            }
            let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(fs::DirEntry::file_name);
            for child in children {
                visit(&child.path(), inventory, false)?;
            }
        } else if metadata.is_file() {
            inventory.files += 1;
            inventory.bytes = inventory
                .bytes
                .checked_add(metadata.len())
                .ok_or_else(|| invalid_data("runtime tree byte count overflow"))?;
        } else {
            inventory.other_entries += 1;
        }
        Ok(())
    }

    let mut inventory = RuntimeTreeInventory::default();
    visit(root, &mut inventory, true)?;
    Ok(inventory)
}

fn tree_has_content(tree: &RuntimeTreeInventory) -> bool {
    tree.files > 0 || tree.directories > 0 || tree.symlinks > 0 || tree.other_entries > 0
}

fn inspect_unmanaged_root_entries(root: &Path) -> io::Result<Vec<String>> {
    let known = [
        "rover.db",
        "rover.db-wal",
        "rover.db-shm",
        "rover.db-journal",
        "objects",
        "tasks",
        "checks",
    ];
    let mut entries = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.retain(|entry| !known.contains(&entry.as_str()));
    entries.sort();
    Ok(entries)
}

fn combined_digest(datasets: &[(&str, &str)]) -> String {
    let mut digest = Sha256::new();
    for (name, value) in datasets {
        update_field(&mut digest, name.as_bytes());
        update_field(&mut digest, value.as_bytes());
    }
    hex_digest(digest.finalize().as_slice())
}

fn update_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(bytes);
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn sqlite_io(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

fn migration_io(error: crate::MigrationError) -> io::Error {
    io::Error::other(error)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::{LegacySchema, LegacyStateInventory};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rover-legacy-inventory-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create root");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                    .expect("make root private");
            }
            Self(path)
        }

        fn create_go_v1(&self) {
            let connection = Connection::open(self.0.join("rover.db")).expect("create database");
            connection
                .execute_batch(
                    "CREATE TABLE records (kind TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id));\
                     CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,ref TEXT NOT NULL,payload TEXT NOT NULL,at TEXT NOT NULL);\
                     CREATE TABLE request_keys (key TEXT PRIMARY KEY,digest TEXT NOT NULL,ref TEXT NOT NULL);\
                     PRAGMA user_version=1;\
                     INSERT INTO records VALUES('task','task_a','{\"objective\":\"fixture\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO events(kind,ref,payload,at) VALUES('task.created','task_a','{\"objective\":\"fixture\"}','2026-01-01T00:00:00Z');\
                     INSERT INTO request_keys VALUES('request_a','digest_a','task_a');",
                )
                .expect("write Go v1 fixture");
        }

        fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
            fn collect(path: &Path, prefix: &str, entries: &mut Vec<(String, Vec<u8>)>) {
                let mut children = fs::read_dir(path)
                    .expect("list source directory")
                    .map(|entry| entry.expect("read directory entry"))
                    .collect::<Vec<_>>();
                children.sort_by_key(fs::DirEntry::file_name);
                for entry in children {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let relative = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    };
                    let metadata = fs::symlink_metadata(entry.path()).expect("stat source entry");
                    if metadata.is_dir() {
                        entries.push((relative.clone(), b"directory".to_vec()));
                        collect(&entry.path(), &relative, entries);
                    } else {
                        entries.push((relative, fs::read(entry.path()).expect("read source file")));
                    }
                }
            }
            let mut entries = Vec::new();
            collect(&self.0, "", &mut entries);
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            entries
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn go_state_inventory_reports_counts_and_never_changes_source() {
        let root = TestRoot::new();
        root.create_go_v1();
        let objects = root.0.join("objects");
        fs::create_dir(&objects).expect("create objects");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&objects, fs::Permissions::from_mode(0o700))
                .expect("make objects private");
        }
        let payload = b"retained artifact";
        let digest = rover_core::Sha256Digest::of(payload).to_hex();
        fs::write(objects.join(&digest), payload).expect("write artifact");
        let task_workspace = root.0.join("tasks/task_a/workspace");
        fs::create_dir_all(&task_workspace).expect("create runtime workspace");
        fs::write(task_workspace.join("log.txt"), b"runtime output").expect("write runtime output");
        fs::write(root.0.join("operator-note.txt"), b"preserve me")
            .expect("write unmanaged source file");

        let before = root.snapshot();
        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect Go state");
        let repeated = LegacyStateInventory::inspect(&root.0).expect("repeat read-only inspection");
        let after = root.snapshot();

        assert_eq!(before, after, "read-only inventory modified the source");
        assert_eq!(inventory.schema, LegacySchema::GoV1);
        assert_eq!(inventory.user_version, 1);
        assert_eq!(inventory.records.rows, 1);
        assert_eq!(inventory.events.rows, 1);
        assert_eq!(inventory.request_keys.rows, 1);
        assert_eq!(inventory.objects.verified, 1);
        assert_eq!(inventory.objects.bytes, payload.len() as u64);
        assert_eq!(inventory.task_tree.files, 1);
        assert_eq!(inventory.task_tree.directories, 2);
        assert_eq!(inventory.task_tree.bytes, b"runtime output".len() as u64);
        assert_eq!(inventory.unmanaged_root_entries, ["operator-note.txt"]);
        assert!(inventory
            .dry_run
            .warnings
            .iter()
            .any(|warning| warning.contains("tasks/") && warning.contains("excluded")));
        assert!(
            inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
        assert_eq!(inventory.dry_run.records, 1);
        assert_eq!(inventory.dry_run.objects, 1);
        assert!(!inventory.dry_run.source_digest.is_empty());
        assert_eq!(
            inventory.dry_run.source_digest,
            repeated.dry_run.source_digest
        );
    }

    #[test]
    fn incompatible_schema_and_tampered_objects_block_dry_run() {
        let root = TestRoot::new();
        root.create_go_v1();
        let objects = root.0.join("objects");
        fs::create_dir(&objects).expect("create objects");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&objects, fs::Permissions::from_mode(0o700))
                .expect("make objects private");
        }
        fs::write(objects.join("a".repeat(64)), b"tampered").expect("write invalid object");
        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect state");
        assert_eq!(inventory.schema, LegacySchema::GoV1);
        assert!(
            !inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
        assert!(inventory
            .dry_run
            .blockers
            .iter()
            .any(|blocker| blocker.contains("digest/size validation")));

        let database = Connection::open(root.0.join("rover.db")).expect("open database");
        database
            .pragma_update(None, "user_version", 42)
            .expect("set unsupported version");
        drop(database);
        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect unsupported state");
        assert_eq!(inventory.schema, LegacySchema::Incompatible);
        assert!(
            !inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
    }

    #[test]
    fn empty_database_is_reported_as_initializable_without_writes() {
        let root = TestRoot::new();
        drop(Connection::open(root.0.join("rover.db")).expect("create empty database"));
        let before = root.snapshot();

        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect empty database");

        assert_eq!(root.snapshot(), before);
        assert_eq!(inventory.schema, LegacySchema::EmptyUnversioned);
        assert!(
            inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
        assert_eq!(inventory.dry_run.records, 0);
        assert_eq!(inventory.dry_run.events, 0);
        assert_eq!(inventory.dry_run.request_keys, 0);
    }

    #[test]
    fn existing_rust_v1_state_is_recognized_read_only() {
        let root = TestRoot::new();
        let mut connection = Connection::open(root.0.join("rover.db")).expect("create database");
        crate::migrate(&mut connection).expect("initialize Rust schema fixture");
        drop(connection);
        let before = root.snapshot();

        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect Rust v1 state");

        assert_eq!(root.snapshot(), before);
        assert_eq!(inventory.schema, LegacySchema::RoverRustV1);
        assert!(
            inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
    }

    #[test]
    fn invalid_record_ids_and_json_are_counted_as_blockers() {
        let root = TestRoot::new();
        root.create_go_v1();
        let connection = Connection::open(root.0.join("rover.db")).expect("open fixture");
        connection
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES(?1,?2,?3,?4)",
                ("bad kind", "bad/id", "{", "2026-01-01T00:00:00Z"),
            )
            .expect("insert malformed record");
        drop(connection);

        let inventory = LegacyStateInventory::inspect(&root.0).expect("inspect malformed state");

        assert_eq!(inventory.dry_run.invalid_record_ids, 1);
        assert_eq!(inventory.dry_run.malformed_record_json, 1);
        assert!(
            !inventory
                .dry_run
                .database_and_objects_importable_after_quiesce
        );
    }

    #[test]
    #[cfg(unix)]
    fn public_state_root_is_refused_without_changing_it() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = TestRoot::new();
        root.create_go_v1();
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).expect("make root public");
        let before = root.snapshot();

        assert!(LegacyStateInventory::inspect(&root.0).is_err());

        assert_eq!(root.snapshot(), before);
    }
}
