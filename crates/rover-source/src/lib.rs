//! Git-backed, content-addressed source capture for the Rover CLI/TUI.
//!
//! Committed captures read Git blobs directly, so checkout filters do not run.
//! Working-tree captures read regular files twice and report that the result is
//! not a filesystem-atomic snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rover_core::{Sha256Digest, Timestamp, SCHEMA};
use serde::{Deserialize, Serialize};

#[cfg(unix)]
mod file_browser;
#[cfg(unix)]
pub use file_browser::{
    browse_directory, quick_open_files, read_repository_file, read_repository_file_for_edit,
    save_repository_file, BrowserEntry, BrowserEntryKind, BrowserGitStatus, BrowserListing,
    MAX_BROWSER_ENTRIES, MAX_FILE_PREVIEW_BYTES,
};

#[cfg(unix)]
use rover_store::files::{SafeDir, StateRoot};

/// Maximum bytes captured for one source file.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum combined source bytes in one snapshot.
pub const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
/// Maximum source file count in one snapshot.
pub const MAX_FILES: usize = 10_000;
const MAX_GIT_STDERR: usize = 1024 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const EXACT_COMMIT_CONSISTENCY: &str = "exact Git commit blobs; no checkout filters";
const WORKTREE_CONSISTENCY: &str =
    "two identical content reads; frozen bytes retained; not a filesystem-atomic capture";
static DIFF_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// File metadata and digest retained in a captured snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotFile {
    /// Slash-separated repository-relative path.
    pub path: String,
    /// Portable permission bits, currently 0644 or 0755.
    pub mode: u32,
    /// Lowercase hexadecimal SHA-256 digest of the captured bytes.
    pub sha256: String,
    /// Exact byte length.
    pub size: i64,
}

/// Stable snapshot metadata stored by Rover.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    /// Rover schema marker.
    pub schema: String,
    /// Stable `snap_<sha256>` identity derived from repository and file list.
    pub id: String,
    /// Canonical repository root.
    pub repository: String,
    /// Commit reference or `WORKTREE`.
    pub source_ref: String,
    /// Resolved base commit identity.
    pub commit: String,
    /// Path-sorted source file inventory.
    pub files: Vec<SnapshotFile>,
    /// RFC3339 UTC capture timestamp.
    pub created_at: String,
    /// Explicit consistency guarantee for this capture.
    pub consistency: String,
}

/// Snapshot metadata plus verified source bytes keyed by SHA-256.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedSnapshot {
    /// Persistable metadata record.
    pub snapshot: Snapshot,
    /// Content-addressed bytes required to persist or materialize the snapshot.
    pub objects: BTreeMap<String, Vec<u8>>,
}

/// One added, deleted, or changed path in a candidate comparison.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Change {
    /// Repository-relative path.
    pub path: String,
    /// `added`, `deleted`, or `modified`.
    pub status: String,
    /// Rover's filename-based risk category.
    pub category: String,
}

/// A deterministic observation about candidate changes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Finding {
    /// Stable rule identifier.
    pub rule: String,
    /// Human-readable explanation.
    pub message: String,
    /// Optional repository-relative path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Evidence classification.
    pub source: String,
}

/// Deterministic base-to-candidate comparison result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Inspection {
    /// Rover schema marker.
    pub schema: String,
    /// Validated base snapshot.
    pub base: Snapshot,
    /// Validated candidate snapshot.
    pub candidate: Snapshot,
    /// Path-sorted observed changes.
    pub changes: Vec<Change>,
    /// Findings derived only from those observed paths and statuses.
    pub findings: Vec<Finding>,
}

/// One registered Git worktree and its porcelain status metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorktreeInfo {
    /// Registered worktree path.
    pub path: String,
    /// Current commit object ID, when Git reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Attached branch reference, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Whether HEAD is detached.
    pub detached: bool,
    /// Whether the entry is a bare repository.
    pub bare: bool,
    /// Lock reason, if the worktree is locked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<String>,
    /// Prune reason, if Git marks the worktree prunable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prunable: Option<String>,
}

/// Observed worktree state compared with its requested frozen base.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorktreeInspection {
    /// Registered worktree metadata.
    pub worktree: WorktreeInfo,
    /// Stable identity of the snapshot used as the inspection baseline.
    pub base_snapshot_id: String,
    /// Captured source manifest, labeled as a two-read non-atomic snapshot.
    pub snapshot: Snapshot,
    /// Path-sorted changes relative to the base snapshot.
    pub changes: Vec<Change>,
    /// Whether any tracked or non-ignored untracked source differs.
    pub dirty: bool,
}

/// Result of verifying that a persisted snapshot record and every referenced
/// immutable content object are present and valid.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotVerification {
    /// Verified snapshot identity.
    pub snapshot_id: String,
    /// Number of metadata entries checked against stored bytes.
    pub verified_files: usize,
    /// Total verified source bytes.
    pub verified_bytes: u64,
}

/// Inputs for committed or working-tree capture.
#[derive(Clone, Debug)]
pub struct CaptureOptions {
    /// Repository path or any path inside a Git worktree.
    pub repository: PathBuf,
    /// State root that will hold snapshot records and blobs in later storage
    /// integration. Capture rejects state roots located inside the repository.
    pub state_root: PathBuf,
    /// Git commit-ish, defaulting to `HEAD`; the exact value `WORKTREE` selects
    /// working-tree capture.
    pub source_ref: String,
    /// Include non-ignored untracked files when capturing `WORKTREE`.
    pub include_untracked: bool,
}

/// Capture failures are fail-closed and include a concise operation context.
#[derive(Debug)]
pub struct CaptureError(String);

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CaptureError {}

impl CaptureError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Capture a committed Git tree or a two-read working-tree snapshot.
///
/// The returned object bytes are immutable for the lifetime of the value. This
/// function never writes to the source repository or state root.
///
/// # Errors
///
/// Fails on invalid roots/references, Git errors, unsupported Git tree entries,
/// unsafe or changing working files, unresolved LFS pointers, and size limits.
pub fn capture(options: &CaptureOptions) -> Result<CapturedSnapshot, CaptureError> {
    if options.state_root.as_os_str().is_empty() {
        return Err(CaptureError::new("state root path is required"));
    }
    let repository = discover(&options.repository)?;
    let state_root = canonicalize_allow_missing(&options.state_root)?;
    if state_root.starts_with(&repository) {
        return Err(CaptureError::new(
            "Rover state root must be outside the repository",
        ));
    }
    let source_ref = if options.source_ref.is_empty() {
        "HEAD"
    } else {
        &options.source_ref
    };
    let first = collect(&repository, source_ref, options.include_untracked)?;
    let (files, objects, consistency) = if source_ref == "WORKTREE" {
        let second = collect(&repository, source_ref, options.include_untracked)?;
        if first.commit != second.commit || first.files != second.files {
            return Err(CaptureError::new(
                "workspace changed during capture; pause writers and retry",
            ));
        }
        (first.files, first.objects, WORKTREE_CONSISTENCY)
    } else {
        (first.files, first.objects, EXACT_COMMIT_CONSISTENCY)
    };
    let repository_text = repository
        .to_str()
        .ok_or_else(|| CaptureError::new("repository root is not valid UTF-8"))?
        .to_owned();
    let source_commit = first.commit;
    let id = snapshot_id(&repository_text, &files)?;
    let created_at = Timestamp::now()
        .to_rfc3339()
        .map_err(|error| CaptureError::new(format!("format capture timestamp: {error}")))?;
    Ok(CapturedSnapshot {
        snapshot: Snapshot {
            schema: SCHEMA.to_owned(),
            id,
            repository: repository_text,
            source_ref: source_ref.to_owned(),
            commit: source_commit,
            files,
            created_at,
            consistency: consistency.to_owned(),
        },
        objects,
    })
}

/// Validate snapshot identity, paths, file metadata, and aggregate limits.
///
/// # Errors
///
/// Returns an error when any identity, path, mode, digest, size, or tree
/// relationship is invalid.
pub fn validate_snapshot(snapshot: &Snapshot) -> Result<(), CaptureError> {
    if snapshot.schema != SCHEMA
        || snapshot.id != snapshot_id(&snapshot.repository, &snapshot.files)?
    {
        return Err(CaptureError::new("snapshot identity mismatch"));
    }
    if !valid_object_id(&snapshot.commit) {
        return Err(CaptureError::new("invalid snapshot commit identity"));
    }
    if snapshot.files.len() > MAX_FILES {
        return Err(CaptureError::new("snapshot has too many files"));
    }
    let max_file_bytes = i64::try_from(MAX_FILE_BYTES)
        .map_err(|_| CaptureError::new("file size limit does not fit snapshot metadata"))?;
    let max_snapshot_bytes = i64::try_from(MAX_SNAPSHOT_BYTES)
        .map_err(|_| CaptureError::new("snapshot size limit does not fit metadata"))?;
    let mut total = 0_i64;
    let mut seen = HashSet::new();
    for file in &snapshot.files {
        validate_relative_path(&file.path)?;
        let folded = file.path.to_lowercase();
        if !seen.insert(folded) {
            return Err(CaptureError::new("duplicate or case-colliding path"));
        }
        if !matches!(file.mode, 0o644 | 0o755) {
            return Err(CaptureError::new("invalid snapshot file mode"));
        }
        if file.size < 0 || file.size > max_file_bytes {
            return Err(CaptureError::new("invalid snapshot file size"));
        }
        total = total
            .checked_add(file.size)
            .ok_or_else(|| CaptureError::new("snapshot size overflow"))?;
        if file.sha256.len() != 64
            || !file
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CaptureError::new("invalid snapshot object digest"));
        }
    }
    if total > max_snapshot_bytes {
        return Err(CaptureError::new("snapshot size budget exceeded"));
    }
    for file in &snapshot.files {
        let components = file.path.split('/').collect::<Vec<_>>();
        for end in 1..components.len() {
            let parent = components[..end].join("/").to_lowercase();
            if seen.contains(&parent) {
                return Err(CaptureError::new("snapshot file/directory conflict"));
            }
        }
    }
    Ok(())
}

/// Compare two validated snapshots and return path-sorted changes and findings.
/// The snapshot IDs bind the repository and path inventory; content and mode
/// differences determine whether each shared path changed.
///
/// # Errors
///
/// Returns an error if either snapshot fails identity, path, metadata, or size
/// validation.
pub fn compare_snapshots(
    base: &Snapshot,
    candidate: &Snapshot,
) -> Result<Inspection, CaptureError> {
    validate_snapshot(base)?;
    validate_snapshot(candidate)?;
    let old = base
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<HashMap<_, _>>();
    let new = candidate
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<HashMap<_, _>>();
    let mut paths = old
        .keys()
        .chain(new.keys())
        .copied()
        .collect::<HashSet<_>>();
    let mut paths = paths.drain().collect::<Vec<_>>();
    paths.sort_unstable();
    let mut inspection = Inspection {
        schema: SCHEMA.to_owned(),
        base: base.clone(),
        candidate: candidate.clone(),
        changes: Vec::new(),
        findings: Vec::new(),
    };
    for path in paths {
        let before = old.get(path);
        let after = new.get(path);
        let status = match (before, after) {
            (None, Some(_)) => "added",
            (Some(_), None) => "deleted",
            (Some(before), Some(after))
                if before.sha256 != after.sha256 || before.mode != after.mode =>
            {
                "modified"
            }
            _ => continue,
        };
        let category = category(path);
        inspection.changes.push(Change {
            path: path.to_owned(),
            status: status.to_owned(),
            category: category.to_owned(),
        });
        if category == "verification-config" || category == "ci" {
            inspection.findings.push(Finding {
                rule: "verification_configuration_changed".to_owned(),
                message: "Verification-related input changed; prior approved policy is retained"
                    .to_owned(),
                path: Some(path.to_owned()),
                source: "observed_diff".to_owned(),
            });
        }
        if category == "test" && status == "deleted" {
            inspection.findings.push(Finding {
                rule: "test_file_deleted".to_owned(),
                message: "A test-classified file was deleted; classification is filename-based"
                    .to_owned(),
                path: Some(path.to_owned()),
                source: "path_heuristic".to_owned(),
            });
        }
    }
    Ok(inspection)
}

/// Compose an explicitly supplied file inventory from already stored,
/// content-addressed objects and persist the resulting immutable snapshot.
///
/// # Errors
///
/// Returns an error if the origin is invalid, the inventory violates snapshot
/// rules, any referenced object is missing/corrupt, or record persistence fails.
#[cfg(unix)]
pub fn compose_snapshot(
    state: &StateRoot,
    origin: &Snapshot,
    mut files: Vec<SnapshotFile>,
    label: &str,
) -> Result<Snapshot, CaptureError> {
    validate_snapshot(origin)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut composed = origin.clone();
    composed.files = files;
    label.clone_into(&mut composed.source_ref);
    composed.created_at = Timestamp::now()
        .to_rfc3339()
        .map_err(|error| CaptureError::new(format!("format composition timestamp: {error}")))?;
    "composed from verified content objects".clone_into(&mut composed.consistency);
    composed.id = snapshot_id(&composed.repository, &composed.files)?;
    validate_snapshot(&composed)?;
    verified_contents(state, &composed)?;
    state
        .records()
        .put("snapshot", &composed.id, &composed, "snapshot.composed")
        .map_err(|error| CaptureError::new(format!("persist composed snapshot: {error}")))?;
    Ok(composed)
}

/// Load a retained snapshot and verify its record identity plus every backing
/// content object. Snapshot records and immutable objects are not pruned by
/// this API; cleanup requires a separately reviewed retention policy.
///
/// # Errors
///
/// Returns an error if the record is absent/mismatched, its metadata is
/// invalid, or any referenced content object is missing/corrupt.
#[cfg(unix)]
pub fn verify_retained_snapshot(
    state: &StateRoot,
    snapshot_id: &str,
) -> Result<SnapshotVerification, CaptureError> {
    let snapshot = state
        .records()
        .get_typed::<Snapshot>("snapshot", snapshot_id)
        .map_err(|error| CaptureError::new(format!("load retained snapshot: {error}")))?;
    validate_snapshot(&snapshot)?;
    if snapshot.id != snapshot_id {
        return Err(CaptureError::new("retained snapshot record ID mismatch"));
    }
    let contents = verified_contents(state, &snapshot)?;
    let verified_bytes = contents.iter().try_fold(0_u64, |total, bytes| {
        total.checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
    });
    let verified_bytes =
        verified_bytes.ok_or_else(|| CaptureError::new("verified snapshot byte count overflow"))?;
    Ok(SnapshotVerification {
        snapshot_id: snapshot.id,
        verified_files: snapshot.files.len(),
        verified_bytes,
    })
}

/// Overlay explicitly selected paths from one exact-base candidate onto a
/// base snapshot. Paths absent from the candidate, duplicates, and an empty
/// path selection are rejected.
///
/// # Errors
///
/// Returns an error for invalid snapshots, repository/base mismatch, invalid
/// or duplicate paths, missing candidate paths, or invalid stored objects.
#[cfg(unix)]
pub fn overlay_snapshot(
    state: &StateRoot,
    base: &Snapshot,
    candidate: &Snapshot,
    paths: &[String],
) -> Result<Snapshot, CaptureError> {
    validate_exact_base_pair(base, candidate)?;
    if paths.is_empty() {
        return Err(CaptureError::new("explicit overlay paths required"));
    }
    let mut files = base
        .files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect::<BTreeMap<_, _>>();
    let candidate_files = candidate
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<HashMap<_, _>>();
    let mut selected = HashSet::new();
    for path in paths {
        validate_relative_path(path)?;
        if !selected.insert(path.as_str()) {
            return Err(CaptureError::new("duplicate overlay path"));
        }
        let file = candidate_files
            .get(path.as_str())
            .ok_or_else(|| CaptureError::new(format!("overlay file absent: {path}")))?;
        files.insert(path.clone(), (*file).clone());
    }
    compose_snapshot(
        state,
        base,
        files.into_values().collect(),
        &format!("overlay:{}", candidate.id),
    )
}

/// Merge multiple exact-base candidate snapshots. Disjoint changes and
/// identical edits compose; different edits to the same path fail closed.
///
/// # Errors
///
/// Returns an error for invalid/mismatched snapshots, conflicting edits, or
/// missing/corrupt stored objects. Conflicts require an explicit integration
/// task and are never resolved by ordering candidates.
#[cfg(unix)]
pub fn merge_snapshots(
    state: &StateRoot,
    base: &Snapshot,
    candidates: &[Snapshot],
) -> Result<Snapshot, CaptureError> {
    validate_snapshot(base)?;
    let mut result = base
        .files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut edits: HashMap<String, Option<SnapshotFile>> = HashMap::new();
    for candidate in candidates {
        validate_exact_base_pair(base, candidate)?;
        let comparison = compare_snapshots(base, candidate)?;
        let candidate_files = candidate
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect::<HashMap<_, _>>();
        for change in comparison.changes {
            let file = candidate_files
                .get(change.path.as_str())
                .map(|file| (*file).clone());
            if let Some(previous) = edits.get(&change.path) {
                if previous != &file {
                    return Err(CaptureError::new(format!(
                        "integration conflict: {}",
                        change.path
                    )));
                }
            }
            edits.insert(change.path.clone(), file.clone());
            if let Some(file) = file {
                result.insert(change.path, file);
            } else {
                result.remove(&change.path);
            }
        }
    }
    compose_snapshot(state, base, result.into_values().collect(), "integration")
}

fn validate_exact_base_pair(base: &Snapshot, candidate: &Snapshot) -> Result<(), CaptureError> {
    validate_snapshot(base)?;
    validate_snapshot(candidate)?;
    if base.repository != candidate.repository || base.commit != candidate.commit {
        return Err(CaptureError::new(
            "candidate repository or exact base commit mismatch",
        ));
    }
    Ok(())
}

fn category(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.starts_with(".github/") || lower.contains(".gitlab-ci") {
        "ci"
    } else if lower.starts_with(".rover/") {
        "verification-config"
    } else if lower.contains("migration") {
        "migration"
    } else if lower.starts_with("docs/")
        || Path::new(&lower)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        "documentation"
    } else if is_test_path(&lower, basename) {
        "test"
    } else if matches!(
        basename,
        "go.mod" | "go.sum" | "package.json" | "requirements.txt" | "pyproject.toml"
    ) || basename.contains("lock")
    {
        "dependency"
    } else if [".yaml", ".yml", ".toml", ".json"]
        .iter()
        .any(|suffix| basename.ends_with(suffix))
    {
        "configuration"
    } else {
        "source"
    }
}

fn is_test_path(path: &str, basename: &str) -> bool {
    path.split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
        || basename.starts_with("test")
        || [
            "_test.go",
            "_test.py",
            ".test.js",
            ".test.ts",
            ".test.jsx",
            ".test.tsx",
            "-test.js",
            "-test.ts",
        ]
        .iter()
        .any(|suffix| basename.ends_with(suffix))
}

/// Produce an applicable binary Git patch using only verified retained bytes.
/// The temporary bare repository has no checkout, filters, hooks, or worktree.
///
/// # Errors
///
/// Returns an error for invalid snapshots, missing/corrupt blobs, unsupported
/// Git patch path framing, a Git failure, or bounded output overflow.
#[cfg(unix)]
pub fn diff(
    state: &StateRoot,
    base: &Snapshot,
    candidate: &Snapshot,
) -> Result<Vec<u8>, CaptureError> {
    validate_snapshot(base)?;
    validate_snapshot(candidate)?;
    let temporary = DiffDirectory::create(state.path())?;
    git(&temporary.path, &["init", "--bare", "--quiet"])?;
    let mut input = Vec::new();
    let mut marks = HashMap::new();
    for snapshot in [base, candidate] {
        for file in &snapshot.files {
            if marks.contains_key(&file.sha256) {
                continue;
            }
            let bytes = state
                .read_blob(&file.sha256)
                .map_err(|error| CaptureError::new(format!("read diff object: {error}")))?;
            if i64::try_from(bytes.len()).unwrap_or(i64::MAX) != file.size {
                return Err(CaptureError::new("diff object size differs from snapshot"));
            }
            let mark = marks.len() + 1;
            marks.insert(file.sha256.clone(), mark);
            writeln!(input, "blob\nmark :{mark}\ndata {}", bytes.len())
                .and_then(|()| input.write_all(&bytes))
                .and_then(|()| input.write_all(b"\n"))
                .map_err(|error| CaptureError::new(format!("frame diff object: {error}")))?;
        }
    }
    for (name, snapshot) in [("base", base), ("candidate", candidate)] {
        writeln!(
            input,
            "commit refs/heads/{name}\ncommitter Rover <rover@localhost> 1 +0000\ndata {}\n{name}",
            name.len()
        )
        .map_err(|error| CaptureError::new(format!("frame diff commit: {error}")))?;
        for file in &snapshot.files {
            if file.path.contains(['"', '\n', '\r']) {
                return Err(CaptureError::new(format!(
                    "unsupported path for diff framing: {:?}",
                    file.path
                )));
            }
            let mode = if file.mode == 0o755 {
                "100755"
            } else {
                "100644"
            };
            let mark = marks
                .get(&file.sha256)
                .ok_or_else(|| CaptureError::new("diff object mark missing"))?;
            writeln!(input, "M {mode} :{mark} \"{}\"", file.path)
                .map_err(|error| CaptureError::new(format!("frame diff path: {error}")))?;
        }
        input.extend_from_slice(b"\n");
    }
    input.extend_from_slice(b"done\n");
    run_fast_import(&temporary.path, &input)?;
    git(
        &temporary.path,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "refs/heads/base",
            "refs/heads/candidate",
            "--",
        ],
    )
}

#[cfg(unix)]
struct DiffDirectory {
    path: PathBuf,
}

#[cfg(unix)]
impl DiffDirectory {
    fn create(root: &Path) -> Result<Self, CaptureError> {
        use std::os::unix::fs::PermissionsExt as _;

        for _ in 0..16 {
            let sequence = DIFF_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("diff-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    if let Err(error) =
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                    {
                        let _ = fs::remove_dir(&path);
                        return Err(CaptureError::new(format!("secure diff directory: {error}")));
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(CaptureError::new(format!("create diff directory: {error}")));
                }
            }
        }
        Err(CaptureError::new(
            "could not allocate unique diff directory",
        ))
    }
}

#[cfg(unix)]
impl Drop for DiffDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn run_fast_import(directory: &Path, input: &[u8]) -> Result<(), CaptureError> {
    let mut command = Command::new("git");
    command.args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-C",
    ]);
    command.arg(directory).args(["fast-import", "--quiet"]);
    command.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .env("HOME", directory)
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| CaptureError::new(format!("start Git fast-import: {error}")))?;
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| CaptureError::new("Git fast-import stdin unavailable"))?
        .write_all(input);
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CaptureError::new(format!("write Git fast-import: {error}")));
    }
    let deadline = Instant::now() + GIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(CaptureError::new(format!(
                    "Git fast-import failed with {status}"
                )));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CaptureError::new("Git fast-import timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CaptureError::new(format!(
                    "wait for Git fast-import: {error}"
                )));
            }
        }
    }
}

/// List Git's registered worktrees using its NUL-delimited porcelain format.
///
/// # Errors
///
/// Returns an error if Git fails or reports malformed/non-UTF-8 metadata.
pub fn list_worktrees(repository: &Path) -> Result<Vec<WorktreeInfo>, CaptureError> {
    let root = discover(repository)?;
    let output = git(&root, &["worktree", "list", "--porcelain", "-z"])?;
    parse_worktree_list(&output)
}

fn parse_worktree_list(output: &[u8]) -> Result<Vec<WorktreeInfo>, CaptureError> {
    let mut worktrees = Vec::new();
    let mut fields = Vec::new();
    for raw_field in output.split(|byte| *byte == 0) {
        if raw_field.is_empty() {
            if !fields.is_empty() {
                worktrees.push(parse_worktree_fields(&fields)?);
                fields.clear();
            }
        } else {
            fields.push(
                std::str::from_utf8(raw_field)
                    .map_err(|_| CaptureError::new("worktree metadata is not UTF-8"))?,
            );
        }
    }
    if !fields.is_empty() {
        worktrees.push(parse_worktree_fields(&fields)?);
    }
    if worktrees.is_empty() {
        return Err(CaptureError::new("Git reported no worktrees"));
    }
    Ok(worktrees)
}

fn parse_worktree_fields(fields: &[&str]) -> Result<WorktreeInfo, CaptureError> {
    let mut path = None;
    let mut head = None;
    let mut branch = None;
    let mut detached = false;
    let mut bare = false;
    let mut locked = None;
    let mut prunable = None;
    for field in fields {
        if let Some(value) = field.strip_prefix("worktree ") {
            if path.replace(value.to_owned()).is_some() || value.is_empty() {
                return Err(CaptureError::new("invalid worktree path metadata"));
            }
        } else if let Some(value) = field.strip_prefix("HEAD ") {
            if !valid_object_id(value) || head.replace(value.to_owned()).is_some() {
                return Err(CaptureError::new("invalid worktree HEAD metadata"));
            }
        } else if let Some(value) = field.strip_prefix("branch ") {
            if value.is_empty() || branch.replace(value.to_owned()).is_some() {
                return Err(CaptureError::new("invalid worktree branch metadata"));
            }
        } else if *field == "detached" {
            detached = true;
        } else if *field == "bare" {
            bare = true;
        } else if *field == "locked" || field.starts_with("locked ") {
            locked = Some(field.strip_prefix("locked ").unwrap_or_default().to_owned());
        } else if *field == "prunable" || field.starts_with("prunable ") {
            prunable = Some(
                field
                    .strip_prefix("prunable ")
                    .unwrap_or_default()
                    .to_owned(),
            );
        } else {
            return Err(CaptureError::new(format!(
                "unsupported worktree metadata field: {field}"
            )));
        }
    }
    Ok(WorktreeInfo {
        path: path.ok_or_else(|| CaptureError::new("worktree path metadata is missing"))?,
        head,
        branch,
        detached,
        bare,
        locked,
        prunable,
    })
}

/// Create a detached worktree at a validated snapshot's commit, then populate
/// its files from immutable stored objects without running checkout filters.
///
/// # Errors
///
/// Returns an error for an invalid snapshot, existing/unsafe destination,
/// failed Git operation, or missing/corrupt materialization blob. A failure
/// after Git registers the worktree leaves that registered path for inspection.
#[cfg(unix)]
pub fn add_worktree(
    state: &StateRoot,
    base: &Snapshot,
    destination: &Path,
) -> Result<(), CaptureError> {
    validate_snapshot(base)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(CaptureError::new("worktree destination must not exist")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CaptureError::new(format!(
                "inspect worktree destination: {error}"
            )));
        }
    }
    let destination = canonicalize_allow_missing(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| CaptureError::new("worktree destination has no parent"))?;
    if !parent.is_dir() {
        return Err(CaptureError::new("worktree destination parent must exist"));
    }
    let _admission_lock = state
        .lock_worktree_admission(&destination)
        .map_err(|error| CaptureError::new(format!("lock worktree destination: {error}")))?;
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(CaptureError::new("worktree destination must not exist")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CaptureError::new(format!(
                "recheck worktree destination: {error}"
            )));
        }
    }
    let destination_text = destination
        .to_str()
        .ok_or_else(|| CaptureError::new("worktree destination is not UTF-8"))?;
    git(
        Path::new(&base.repository),
        &[
            "worktree",
            "add",
            "--detach",
            "--no-checkout",
            destination_text,
            &base.commit,
        ],
    )?;
    if let Err(error) = git(&destination, &["read-tree", &base.commit]) {
        return Err(CaptureError::new(format!(
            "worktree registered but index initialization failed: {error}"
        )));
    }
    if let Err(error) = materialize(state, base, &destination) {
        return Err(CaptureError::new(format!(
            "worktree registered but snapshot materialization failed: {error}"
        )));
    }
    Ok(())
}

/// Replace a newly created task worktree's base files with a same-repository
/// initial snapshot while preserving the exact base commit in Git's HEAD.
/// Every initial object and all preconditions are checked before any base file
/// is removed. File/directory shape changes that could block safe exclusive
/// materialization are rejected without modifying the worktree.
///
/// # Errors
///
/// Returns an error for repository mismatch, a non-task/unregistered worktree,
/// a HEAD other than the frozen base commit, dirty worktree contents, invalid
/// snapshot structure, conflicting file/directory layouts, or missing/corrupt
/// content objects.
#[cfg(unix)]
pub fn replace_worktree_snapshot(
    state: &StateRoot,
    base: &Snapshot,
    initial: &Snapshot,
    worktree_path: &Path,
) -> Result<(), CaptureError> {
    validate_snapshot(base)?;
    validate_snapshot(initial)?;
    if initial.repository != base.repository {
        return Err(CaptureError::new("input snapshot repository mismatch"));
    }
    validate_replacement_layout(&base.files, &initial.files)?;
    let contents = verified_contents(state, initial)?;

    let worktree = find_worktree(Path::new(&base.repository), worktree_path)?;
    if worktree.head.as_deref() != Some(base.commit.as_str()) {
        return Err(CaptureError::new(
            "task worktree HEAD does not match frozen base commit",
        ));
    }
    let canonical_worktree = Path::new(&worktree.path);
    let tasks_storage = fs::canonicalize(state.path().join("tasks"))
        .map_err(|error| CaptureError::new(format!("resolve Rover task root: {error}")))?;
    let task_directory = canonical_worktree
        .parent()
        .ok_or_else(|| CaptureError::new("worktree path has no task directory"))?;
    let task_directory_parent = task_directory
        .parent()
        .ok_or_else(|| CaptureError::new("worktree path is outside task storage"))?;
    if task_directory_parent != tasks_storage
        || canonical_worktree.file_name() != Some(std::ffi::OsStr::new("workspace"))
        || task_directory
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| rover_core::Id::parse(name.to_owned()).ok())
            .is_none()
    {
        return Err(CaptureError::new(
            "workspace replacement is restricted to a Rover task worktree",
        ));
    }
    let status = git(
        canonical_worktree,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    if !status.is_empty() {
        return Err(CaptureError::new(
            "base worktree changed before initial snapshot replacement",
        ));
    }

    let root = SafeDir::open_directory(canonical_worktree)
        .map_err(|error| CaptureError::new(format!("open task worktree: {error}")))?;
    for file in &base.files {
        root.remove_regular_file_path(&file.path).map_err(|error| {
            CaptureError::new(format!("remove frozen base file {}: {error}", file.path))
        })?;
    }
    write_materialized(&root, initial, contents)
}

fn validate_replacement_layout(
    base_files: &[SnapshotFile],
    initial_files: &[SnapshotFile],
) -> Result<(), CaptureError> {
    let base_paths = base_files
        .iter()
        .map(|file| file.path.to_lowercase())
        .collect::<HashSet<_>>();
    let initial_paths = initial_files
        .iter()
        .map(|file| file.path.to_lowercase())
        .collect::<HashSet<_>>();
    for (files, other_paths) in [(base_files, &initial_paths), (initial_files, &base_paths)] {
        for file in files {
            let components = file.path.split('/').collect::<Vec<_>>();
            for end in 1..components.len() {
                let parent = components[..end].join("/").to_lowercase();
                if other_paths.contains(&parent) {
                    return Err(CaptureError::new(
                        "base and initial snapshots have incompatible file/directory layouts",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Inspect a registered worktree against its frozen base snapshot. Working
/// files are read twice and untracked non-ignored files are included.
///
/// # Errors
///
/// Returns an error if the path is not a registered worktree or source capture
/// encounters unsafe, changing, or unsupported inputs.
#[cfg(unix)]
pub fn inspect_worktree(
    state: &StateRoot,
    base: &Snapshot,
    path: &Path,
) -> Result<WorktreeInspection, CaptureError> {
    validate_snapshot(base)?;
    let info = find_worktree(Path::new(&base.repository), path)?;
    let snapshot = capture(&CaptureOptions {
        repository: PathBuf::from(&info.path),
        state_root: state.path().to_path_buf(),
        source_ref: "WORKTREE".to_owned(),
        include_untracked: true,
    })?
    .snapshot;
    let comparison = compare_snapshots(base, &snapshot)?;
    let dirty = !comparison.changes.is_empty();
    Ok(WorktreeInspection {
        worktree: info,
        base_snapshot_id: base.id.clone(),
        snapshot,
        changes: comparison.changes,
        dirty,
    })
}

/// Resolve a branch, tag, or commit-ish to a full immutable commit object ID.
/// Options and non-commit objects are rejected before they can become a base.
///
/// # Errors
///
/// Returns an error for an invalid repository/reference or when Git cannot
/// resolve the reference to a commit.
pub fn resolve_reference(repository: &Path, reference: &str) -> Result<String, CaptureError> {
    let repository = discover(repository)?;
    resolve(&repository, reference)
}

/// Derive a stable Rover resource identifier for serializing work on one local
/// branch. The repository's canonical path and fully qualified branch ref are
/// both part of the digest, so same-named branches in separate repositories do
/// not contend.
///
/// Pass the returned ID as a named resource to `Store::acquire_resources`.
///
/// # Errors
///
/// Returns an error for an invalid repository or Git branch name.
pub fn branch_reservation_resource(
    repository: &Path,
    branch: &str,
) -> Result<String, CaptureError> {
    if branch.is_empty() || branch.starts_with('-') || branch.contains('\0') || branch.len() > 1024
    {
        return Err(CaptureError::new("invalid Git branch name"));
    }
    let repository = discover(repository)?;
    let qualified = format!("refs/heads/{branch}");
    git(&repository, &["check-ref-format", &qualified])
        .map_err(|error| CaptureError::new(format!("invalid Git branch name: {error}")))?;
    let repository_bytes = repository.as_os_str().as_encoded_bytes();
    let mut framed = Vec::with_capacity(repository_bytes.len() + branch.len() + 24);
    framed.extend_from_slice(b"rover-branch-reservation/v1:");
    framed.extend_from_slice(repository_bytes);
    framed.push(0);
    framed.extend_from_slice(qualified.as_bytes());
    Ok(format!("branch-{}", Sha256Digest::of(&framed)))
}

/// Remove a registered worktree only when its tracked, untracked, and ignored
/// contents are all clean. Git performs the final non-forced removal check.
///
/// # Errors
///
/// Returns an error for the main worktree, unknown/unsafe path, dirty contents,
/// or any Git removal failure.
pub fn remove_worktree(repository: &Path, path: &Path) -> Result<(), CaptureError> {
    let root = discover(repository)?;
    let info = find_worktree(&root, path)?;
    if Path::new(&info.path) == root {
        return Err(CaptureError::new("refusing to remove the main worktree"));
    }
    let status = git(
        Path::new(&info.path),
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    if !status.is_empty() {
        return Err(CaptureError::new(
            "refusing to remove a worktree with modified, untracked, or ignored files",
        ));
    }
    let path_argument = info.path.as_str();
    git(&root, &["worktree", "remove", "--", path_argument])?;
    if list_worktrees(&root)?
        .iter()
        .any(|entry| entry.path == info.path)
    {
        return Err(CaptureError::new("Git still reports removed worktree"));
    }
    Ok(())
}

fn find_worktree(repository: &Path, path: &Path) -> Result<WorktreeInfo, CaptureError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CaptureError::new(format!("inspect worktree path: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CaptureError::new("worktree path must be a real directory"));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| CaptureError::new(format!("resolve worktree path: {error}")))?;
    let canonical = canonical
        .to_str()
        .ok_or_else(|| CaptureError::new("worktree path is not UTF-8"))?;
    list_worktrees(repository)?
        .into_iter()
        .find(|worktree| worktree.path == canonical)
        .ok_or_else(|| CaptureError::new("path is not a registered worktree"))
}

/// Confirm that materialized source files still have the captured bytes and
/// executable mode.
///
/// # Errors
///
/// Returns an error if the snapshot is invalid or an input is missing, unsafe,
/// modified, or has a different executable mode.
pub fn inputs_unchanged(snapshot: &Snapshot, directory: &Path) -> Result<(), CaptureError> {
    validate_snapshot(snapshot)?;
    for expected in &snapshot.files {
        let (bytes, mode) = read_regular(directory, &expected.path).map_err(|error| {
            CaptureError::new(format!(
                "input {} missing or unsafe: {error}",
                expected.path
            ))
        })?;
        let digest = Sha256Digest::of(&bytes).to_hex();
        if digest != expected.sha256
            || i64::try_from(bytes.len()).unwrap_or(i64::MAX) != expected.size
            || mode != expected.mode
        {
            return Err(CaptureError::new(format!(
                "check modified source input {}",
                expected.path
            )));
        }
    }
    Ok(())
}

/// Verify all referenced blobs, then materialize files exclusively under a
/// private output root. No checkout filter or repository program is executed.
///
/// # Errors
///
/// Returns an error for an invalid snapshot, missing or corrupt objects, an
/// unsafe destination or parent, or an output file that already exists.
#[cfg(unix)]
pub fn materialize(
    state: &StateRoot,
    snapshot: &Snapshot,
    destination: &Path,
) -> Result<(), CaptureError> {
    validate_snapshot(snapshot)?;
    let contents = verified_contents(state, snapshot)?;
    let root = prepare_materialization_root(destination)?;
    write_materialized(&root, snapshot, contents)
}

#[cfg(unix)]
fn verified_contents(state: &StateRoot, snapshot: &Snapshot) -> Result<Vec<Vec<u8>>, CaptureError> {
    let mut contents = Vec::with_capacity(snapshot.files.len());
    for file in &snapshot.files {
        let bytes = state
            .read_blob(&file.sha256)
            .map_err(|error| CaptureError::new(format!("read snapshot object: {error}")))?;
        if i64::try_from(bytes.len()).unwrap_or(i64::MAX) != file.size
            || Sha256Digest::of(&bytes).to_hex() != file.sha256
        {
            return Err(CaptureError::new("snapshot size or digest mismatch"));
        }
        contents.push(bytes);
    }
    Ok(contents)
}

#[cfg(unix)]
fn write_materialized(
    root: &SafeDir,
    snapshot: &Snapshot,
    contents: Vec<Vec<u8>>,
) -> Result<(), CaptureError> {
    for (file, bytes) in snapshot.files.iter().zip(contents) {
        let directory = root
            .ensure_subdirectory_path(&file.path)
            .map_err(|error| CaptureError::new(format!("create source directory: {error}")))?;
        let name = file
            .path
            .rsplit('/')
            .next()
            .ok_or_else(|| CaptureError::new("snapshot path has no filename"))?;
        directory
            .create_new_file(name, &bytes, file.mode)
            .map_err(|error| CaptureError::new(format!("materialize {}: {error}", file.path)))?;
    }
    Ok(())
}

#[cfg(unix)]
fn prepare_materialization_root(path: &Path) -> Result<SafeDir, CaptureError> {
    let root_path = canonicalize_allow_missing(path)?;
    if root_path.parent().is_none() {
        return Err(CaptureError::new(
            "materialization destination must be a dedicated directory",
        ));
    }
    fs::create_dir_all(&root_path)
        .map_err(|error| CaptureError::new(format!("create materialization root: {error}")))?;
    let root = SafeDir::open_directory(&root_path)
        .map_err(|error| CaptureError::new(format!("open materialization root: {error}")))?;
    root.set_permissions(0o700).map_err(|error| {
        CaptureError::new(format!("make materialization root private: {error}"))
    })?;
    Ok(root)
}

/// Capture and persist immutable objects plus the snapshot record.
///
/// This adapter is available where Rover's admitted `StateRoot` filesystem
/// implementation is available. Blob publication precedes the transactional
/// snapshot record, so a failed record write can leave only unreachable,
/// content-addressed objects.
///
/// # Errors
///
/// Returns capture errors or storage errors. A published object digest must
/// match the metadata computed from the captured bytes.
#[cfg(unix)]
pub fn capture_into_state(
    options: &CaptureOptions,
    state: &rover_store::files::StateRoot,
) -> Result<Snapshot, CaptureError> {
    let mut state_bound_options = options.clone();
    state_bound_options.state_root = state.path().to_path_buf();
    let captured = capture(&state_bound_options)?;
    for (expected_digest, bytes) in &captured.objects {
        let actual_digest = state
            .put_blob(bytes)
            .map_err(|error| CaptureError::new(format!("publish source object: {error}")))?
            .to_hex();
        if &actual_digest != expected_digest {
            return Err(CaptureError::new(
                "published source object digest differs from captured metadata",
            ));
        }
    }
    state
        .records()
        .put(
            "snapshot",
            &captured.snapshot.id,
            &captured.snapshot,
            "snapshot.captured",
        )
        .map_err(|error| CaptureError::new(format!("persist source snapshot: {error}")))?;
    Ok(captured.snapshot)
}

struct CaptureBody {
    commit: String,
    files: Vec<SnapshotFile>,
    objects: BTreeMap<String, Vec<u8>>,
}

fn collect(
    repository: &Path,
    source_ref: &str,
    include_untracked: bool,
) -> Result<CaptureBody, CaptureError> {
    let head = resolve(repository, "HEAD")?;
    let mut body = CaptureBody {
        commit: head,
        files: Vec::new(),
        objects: BTreeMap::new(),
    };
    let mut seen = HashSet::new();
    let mut total = 0_usize;
    if source_ref == "WORKTREE" {
        let staged = git(repository, &["ls-files", "--stage", "-z"])?;
        if staged
            .split(|byte| *byte == 0)
            .any(|record| record.starts_with(b"160000 "))
        {
            return Err(CaptureError::new(
                "submodules are unsupported in this alpha",
            ));
        }
        let mut args = vec!["ls-files", "-z", "--cached"];
        if include_untracked {
            args.extend(["--others", "--exclude-standard"]);
        }
        let output = git(repository, &args)?;
        let mut unique = HashSet::new();
        for raw_path in output
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let path = utf8_path(raw_path)?;
            if !unique.insert(path.to_owned()) || path.starts_with(".rover-results/") {
                continue;
            }
            match read_regular(repository, path) {
                Ok((bytes, mode)) => add_file(&mut body, &mut seen, &mut total, path, mode, bytes)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CaptureError::new(format!("read {path}: {error}"))),
            }
        }
    } else {
        body.commit = resolve(repository, source_ref)?;
        let tree = git(repository, &["ls-tree", "-rz", "--full-tree", &body.commit])?;
        for record in tree
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let separator = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| CaptureError::new("malformed Git tree entry"))?;
            let header = std::str::from_utf8(&record[..separator])
                .map_err(|_| CaptureError::new("malformed Git tree header"))?;
            let path = utf8_path(&record[separator + 1..])?;
            let mut fields = header.split_ascii_whitespace();
            let mode = fields.next().unwrap_or_default();
            let object_type = fields.next().unwrap_or_default();
            let object_id = fields.next().unwrap_or_default();
            if fields.next().is_some() {
                return Err(CaptureError::new("malformed Git tree entry"));
            }
            let file_mode = match mode {
                "100644" => 0o644,
                "100755" => 0o755,
                other => {
                    return Err(CaptureError::new(format!(
                        "unsupported Git entry mode {other} at {path} (symlink/submodule)"
                    )))
                }
            };
            if object_type != "blob" || !valid_object_id(object_id) {
                return Err(CaptureError::new("invalid Git blob identity"));
            }
            let size = git(repository, &["cat-file", "-s", object_id])?;
            let size_text = std::str::from_utf8(&size)
                .map_err(|_| CaptureError::new("Git blob size is not UTF-8"))?
                .trim();
            let size = size_text
                .parse::<usize>()
                .map_err(|_| CaptureError::new("invalid Git blob size"))?;
            if size > MAX_FILE_BYTES {
                return Err(CaptureError::new(format!("oversized Git blob: {path}")));
            }
            let bytes = git(repository, &["cat-file", "blob", object_id])?;
            if bytes.len() != size {
                return Err(CaptureError::new(format!("Git blob size changed: {path}")));
            }
            add_file(&mut body, &mut seen, &mut total, path, file_mode, bytes)?;
        }
    }
    body.files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(body)
}

fn add_file(
    body: &mut CaptureBody,
    seen: &mut HashSet<String>,
    total: &mut usize,
    path: &str,
    mode: u32,
    bytes: Vec<u8>,
) -> Result<(), CaptureError> {
    validate_relative_path(path)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(CaptureError::new(format!("file exceeds 8 MiB: {path}")));
    }
    if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
        return Err(CaptureError::new(format!(
            "unresolved Git LFS pointer: {path}"
        )));
    }
    let folded = path.to_lowercase();
    if !seen.insert(folded) {
        return Err(CaptureError::new(format!(
            "duplicate or case-colliding source path: {path}"
        )));
    }
    *total = total
        .checked_add(bytes.len())
        .ok_or_else(|| CaptureError::new("snapshot byte count overflow"))?;
    if *total > MAX_SNAPSHOT_BYTES {
        return Err(CaptureError::new("snapshot exceeds 128 MiB"));
    }
    if body.files.len() >= MAX_FILES {
        return Err(CaptureError::new("snapshot exceeds 10000 files"));
    }
    let digest = Sha256Digest::of(&bytes).to_hex();
    body.files.push(SnapshotFile {
        path: path.to_owned(),
        mode,
        sha256: digest.clone(),
        size: i64::try_from(bytes.len())
            .map_err(|_| CaptureError::new("file size does not fit snapshot schema"))?,
    });
    body.objects.insert(digest, bytes);
    Ok(())
}

fn snapshot_id(repository: &str, files: &[SnapshotFile]) -> Result<String, CaptureError> {
    #[derive(Serialize)]
    struct Identity<'a> {
        #[serde(rename = "Repository")]
        repository: &'a str,
        #[serde(rename = "Files")]
        files: &'a [SnapshotFile],
    }
    let json = serde_json::to_vec(&Identity { repository, files })
        .map_err(|error| CaptureError::new(format!("serialize snapshot identity: {error}")))?;
    // Go's encoding/json escapes HTML and the two JavaScript line separators.
    // The model's identity hash depends on those exact bytes.
    let json = go_json_escape(&json);
    Ok(format!("snap_{}", Sha256Digest::of(&json).to_hex()))
}

fn go_json_escape(json: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(json);
    text.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes()
}

fn discover(repository: &Path) -> Result<PathBuf, CaptureError> {
    let path = if repository.as_os_str().is_empty() {
        std::env::current_dir().map_err(|error| CaptureError::new(error.to_string()))?
    } else {
        lexical_absolute(repository)?
    };
    let output = git(&path, &["rev-parse", "--show-toplevel"])?;
    let root = std::str::from_utf8(&output)
        .map_err(|_| CaptureError::new("repository root is not UTF-8"))?
        .strip_suffix('\n')
        .ok_or_else(|| CaptureError::new("Git returned an unterminated repository path"))?;
    if root.contains(['\r', '\n']) {
        return Err(CaptureError::new(
            "repository root containing a newline is unsupported",
        ));
    }
    fs::canonicalize(root).map_err(|error| CaptureError::new(error.to_string()))
}

fn resolve(repository: &Path, reference: &str) -> Result<String, CaptureError> {
    if reference.is_empty()
        || reference.starts_with('-')
        || reference.contains('\0')
        || reference.len() > 1024
    {
        return Err(CaptureError::new("invalid Git reference"));
    }
    let output = git(
        repository,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    let object_id = std::str::from_utf8(&output)
        .map_err(|_| CaptureError::new("Git returned a non-UTF-8 commit identity"))?
        .trim();
    if !valid_object_id(object_id) {
        return Err(CaptureError::new("Git returned an invalid commit identity"));
    }
    Ok(object_id.to_owned())
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn utf8_path(bytes: &[u8]) -> Result<&str, CaptureError> {
    std::str::from_utf8(bytes).map_err(|_| CaptureError::new("repository path is not valid UTF-8"))
}

fn git(repository: &Path, arguments: &[&str]) -> Result<Vec<u8>, CaptureError> {
    if arguments.is_empty() {
        return Err(CaptureError::new("Git requires at least one argument"));
    }
    let mut command = Command::new("git");
    command.args([
        "--no-optional-locks",
        "-c",
        &format!("core.hooksPath={}", null_device()),
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "core.pager=cat",
        "-c",
        "color.ui=false",
        "-c",
        "protocol.file.allow=never",
        "-C",
    ]);
    command.arg(repository).args(arguments);
    command.env_clear();
    for (key, value) in std::env::vars_os() {
        if !key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env(key, value);
        }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_GLOBAL", null_device());
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    command.env("GIT_OPTIONAL_LOCKS", "0");
    command.env("LC_ALL", "C");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| CaptureError::new(format!("start Git: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CaptureError::new("Git stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CaptureError::new("Git stderr pipe unavailable"))?;
    let stdout_reader = thread::spawn(move || read_limited(stdout, MAX_SNAPSHOT_BYTES));
    let stderr_reader = thread::spawn(move || read_limited(stderr, MAX_GIT_STDERR));
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(CaptureError::new("Git command timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(CaptureError::new(format!("wait for Git: {error}")));
            }
        }
    };
    let output = stdout_reader
        .join()
        .map_err(|_| CaptureError::new("Git stdout reader panicked"))?
        .map_err(|error| CaptureError::new(format!("read Git stdout: {error}")))?;
    let error_output = stderr_reader
        .join()
        .map_err(|_| CaptureError::new("Git stderr reader panicked"))?
        .map_err(|error| CaptureError::new(format!("read Git stderr: {error}")))?;
    if !status.success() {
        return Err(CaptureError::new(format!(
            "git {} failed: {}",
            arguments[0],
            String::from_utf8_lossy(&error_output).trim()
        )));
    }
    Ok(output)
}

fn read_limited(reader: impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut output = Vec::new();
    reader.take(limit).read_to_end(&mut output)?;
    if output.len() > maximum {
        return Err(io::Error::other("Git output exceeded capture limit"));
    }
    Ok(output)
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, CaptureError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| CaptureError::new(error.to_string()))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn canonicalize_allow_missing(path: &Path) -> Result<PathBuf, CaptureError> {
    let absolute = lexical_absolute(path)?;
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing
                    .file_name()
                    .ok_or_else(|| CaptureError::new("path has no existing ancestor"))?;
                suffix.push(name.to_os_string());
                existing = existing
                    .parent()
                    .ok_or_else(|| CaptureError::new("path has no existing ancestor"))?;
            }
            Err(error) => return Err(CaptureError::new(format!("inspect path: {error}"))),
        }
    }
    let mut ancestor = Some(existing);
    while let Some(candidate) = ancestor {
        let metadata = fs::symlink_metadata(candidate)
            .map_err(|error| CaptureError::new(format!("inspect path ancestor: {error}")))?;
        if metadata.file_type().is_symlink() && !is_platform_path_symlink(candidate) {
            return Err(CaptureError::new(format!(
                "unsafe symlink in path ancestor: {}",
                candidate.display()
            )));
        }
        ancestor = candidate.parent();
    }
    let canonical = fs::canonicalize(existing)
        .map_err(|error| CaptureError::new(format!("resolve path: {error}")))?;
    let mut resolved = canonical;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn is_platform_path_symlink(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        path == Path::new("/tmp") || path == Path::new("/var")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

fn validate_relative_path(path: &str) -> Result<(), CaptureError> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || candidate.to_string_lossy() != path
    {
        return Err(CaptureError::new(format!(
            "unsafe repository path {path:?}"
        )));
    }
    for component in path.split('/') {
        if component.eq_ignore_ascii_case(".git") {
            return Err(CaptureError::new("nested .git paths are unsupported"));
        }
    }
    Ok(())
}

fn read_regular(repository: &Path, path: &str) -> io::Result<(Vec<u8>, u32)> {
    validate_relative_path(path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut current = repository.to_path_buf();
    let parts = path.split('/').collect::<Vec<_>>();
    for part in &parts {
        current.push(part);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "symlink source unsupported",
            ));
        }
    }
    let metadata = fs::symlink_metadata(&current)?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "special source file unsupported",
        ));
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source file exceeds 8 MiB",
        ));
    }
    let mut file = File::open(&current)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_FILE_BYTES as u64) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file grew past 8 MiB",
        ));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 != 0 {
            0o755
        } else {
            0o644
        }
    };
    #[cfg(not(unix))]
    let mode = 0o644;
    Ok((bytes, mode))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        add_file, add_worktree, branch_reservation_resource, capture, compare_snapshots,
        compose_snapshot, diff, inputs_unchanged, inspect_worktree, list_worktrees, materialize,
        merge_snapshots, overlay_snapshot, remove_worktree, replace_worktree_snapshot,
        resolve_reference, validate_relative_path, validate_snapshot, verify_retained_snapshot,
        CaptureBody, CaptureOptions, SnapshotFile, EXACT_COMMIT_CONSISTENCY, MAX_FILES,
        MAX_FILE_BYTES, MAX_SNAPSHOT_BYTES, WORKTREE_CONSISTENCY,
    };
    use rover_core::Sha256Digest;
    use std::collections::{BTreeMap, HashSet};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Repo {
        container: PathBuf,
        root: PathBuf,
    }

    impl Repo {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let container = std::env::temp_dir().join(format!(
                "rover-source-test-{}-{timestamp}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&container).expect("create fixture container");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&container, fs::Permissions::from_mode(0o700))
                    .expect("make fixture container private");
            }
            let root = container.join("repo");
            fs::create_dir(&root).expect("create repository");
            let repo = Self { container, root };
            repo.git(&["init", "-q"]);
            repo.git(&["config", "user.email", "rover@example.invalid"]);
            repo.git(&["config", "user.name", "Rover Test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }

        fn file(&self, name: &str, contents: &[u8]) {
            let path = self.root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create nested parent");
            }
            fs::write(path, contents).expect("write fixture file");
        }

        fn commit_file(&self, name: &str, contents: &[u8]) {
            self.file(name, contents);
            self.git(&["add", "--", name]);
            self.git(&["commit", "-q", "-m", "fixture"]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.container);
        }
    }

    fn options(repo: &Path, source_ref: &str, include_untracked: bool) -> CaptureOptions {
        CaptureOptions {
            repository: repo.to_path_buf(),
            state_root: repo.parent().unwrap().join("state"),
            source_ref: source_ref.to_owned(),
            include_untracked,
        }
    }

    #[test]
    fn committed_capture_reads_exact_git_blobs_and_labels_consistency() {
        let repo = Repo::new();
        repo.commit_file("src/value.txt", b"committed bytes");
        let commit = repo.git(&["rev-parse", "HEAD"]);
        repo.file("src/value.txt", b"working bytes");

        let captured = capture(&options(&repo.root, "HEAD", false)).expect("capture commit");

        assert_eq!(captured.snapshot.commit, commit);
        assert_eq!(captured.snapshot.consistency, EXACT_COMMIT_CONSISTENCY);
        assert_eq!(captured.snapshot.files.len(), 1);
        let file = &captured.snapshot.files[0];
        assert_eq!(file.path, "src/value.txt");
        assert_eq!(
            file.size,
            i64::try_from(b"committed bytes".len()).expect("fixture length fits i64")
        );
        assert_eq!(
            captured.objects.get(&file.sha256).unwrap(),
            b"committed bytes"
        );
        assert_eq!(captured.snapshot.source_ref, "HEAD");
    }

    #[test]
    fn committed_capture_does_not_run_checkout_filters() {
        let repo = Repo::new();
        let marker = repo.container.join("filter-ran");
        repo.file(".gitattributes", b"data.txt filter=probe\n");
        repo.file("data.txt", b"committed source bytes");
        repo.git(&["add", "--", ".gitattributes", "data.txt"]);
        repo.git(&[
            "config",
            "filter.probe.smudge",
            &format!("touch {} && cat", marker.display()),
        ]);
        repo.git(&["commit", "-q", "-m", "filtered fixture"]);

        let captured = capture(&options(&repo.root, "HEAD", false)).expect("capture commit");

        assert_eq!(captured.snapshot.files.len(), 2);
        let data = captured
            .snapshot
            .files
            .iter()
            .find(|file| file.path == "data.txt")
            .unwrap();
        assert_eq!(
            captured.objects.get(&data.sha256).unwrap(),
            b"committed source bytes"
        );
        assert!(!marker.exists(), "Git checkout filter unexpectedly ran");
    }

    #[test]
    fn working_capture_labels_two_reads_and_requires_untracked_opt_in() {
        let repo = Repo::new();
        repo.commit_file("tracked.txt", b"base");
        repo.file("tracked.txt", b"working");
        repo.file("new.txt", b"untracked");

        let without_untracked = capture(&options(&repo.root, "WORKTREE", false))
            .expect("capture tracked working files");
        assert_eq!(without_untracked.snapshot.consistency, WORKTREE_CONSISTENCY);
        assert_eq!(without_untracked.snapshot.files.len(), 1);
        assert_eq!(
            without_untracked
                .objects
                .get(&without_untracked.snapshot.files[0].sha256)
                .unwrap(),
            b"working"
        );

        let with_untracked = capture(&options(&repo.root, "WORKTREE", true))
            .expect("capture opted-in untracked files");
        assert_eq!(with_untracked.snapshot.files.len(), 2);
        assert_eq!(with_untracked.snapshot.files[0].path, "new.txt");
        assert_eq!(with_untracked.snapshot.files[1].path, "tracked.txt");
        assert_ne!(without_untracked.snapshot.id, with_untracked.snapshot.id);
    }

    #[test]
    fn capture_refuses_state_root_inside_repository() {
        let repo = Repo::new();
        repo.commit_file("file.txt", b"bytes");
        let mut capture_options = options(&repo.root, "HEAD", false);
        capture_options.state_root = repo.root.join(".rover");

        assert!(capture(&capture_options).is_err());

        capture_options.state_root = repo.container.clone();
        assert!(
            capture(&capture_options).is_ok(),
            "Go contract permits an ancestor state root outside the repository"
        );
    }

    #[test]
    fn snapshot_identity_bytes_match_go_json_field_names_and_html_escaping() {
        let files = [super::SnapshotFile {
            path: "<source>.rs".to_owned(),
            mode: 0o644,
            sha256: "ab".repeat(32),
            size: 3,
        }];
        let go_identity_json = format!(
            r#"{{"Repository":"repo\u003c\u0026\u003e","Files":[{{"path":"\u003csource\u003e.rs","mode":420,"sha256":"{}","size":3}}]}}"#,
            "ab".repeat(32)
        );
        let expected = format!(
            "snap_{}",
            Sha256Digest::of(go_identity_json.as_bytes()).to_hex()
        );

        assert_eq!(super::snapshot_id("repo<&>", &files).unwrap(), expected);
    }

    #[test]
    fn repository_path_validation_rejects_escape_and_noncanonical_forms() {
        for path in [
            "",
            ".",
            "../outside",
            "dir/../outside",
            "dir/./file",
            "dir//file",
            "/absolute",
            "dir\\file",
            "dir/.git/config",
            "dir/.GIT/config",
        ] {
            assert!(
                validate_relative_path(path).is_err(),
                "unsafe path unexpectedly accepted: {path:?}"
            );
        }
        assert!(validate_relative_path("src/module.rs").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn working_capture_rejects_symlink_sources() {
        let repo = Repo::new();
        repo.commit_file("tracked.txt", b"base");
        let outside = repo.container.join("outside.txt");
        fs::write(&outside, b"outside bytes").unwrap();
        std::os::unix::fs::symlink(&outside, repo.root.join("link.txt")).unwrap();
        repo.git(&["add", "--", "link.txt"]);

        assert!(capture(&options(&repo.root, "WORKTREE", false)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn committed_capture_rejects_git_symlink_entries() {
        let repo = Repo::new();
        repo.file("target.txt", b"target");
        std::os::unix::fs::symlink("target.txt", repo.root.join("link.txt")).unwrap();
        repo.git(&["add", "--", "target.txt", "link.txt"]);
        repo.git(&["commit", "-q", "-m", "symlink fixture"]);

        let error = capture(&options(&repo.root, "HEAD", false)).unwrap_err();
        assert!(error.to_string().contains("symlink/submodule"));
    }

    #[test]
    fn capture_rejects_unresolved_lfs_pointer_files() {
        let repo = Repo::new();
        repo.commit_file(
            "pointer.dat",
            b"version https://git-lfs.github.com/spec/v1\noid sha256:deadbeef\nsize 1\n",
        );

        let error = capture(&options(&repo.root, "HEAD", false)).unwrap_err();
        assert!(error.to_string().contains("unresolved Git LFS pointer"));
    }

    #[test]
    fn working_capture_rejects_gitlinks_before_reading_worktree_paths() {
        let repo = Repo::new();
        repo.commit_file("tracked.txt", b"base");
        let child_commit = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{child_commit},modules/child"),
        ]);

        let error = capture(&options(&repo.root, "WORKTREE", false)).unwrap_err();
        assert!(error.to_string().contains("submodules are unsupported"));

        repo.git(&["commit", "-q", "-m", "gitlink fixture"]);
        let error = capture(&options(&repo.root, "HEAD", false)).unwrap_err();
        assert!(error
            .to_string()
            .contains("unsupported Git entry mode 160000"));
    }

    #[test]
    fn capture_rejects_case_collisions_when_filesystem_can_represent_them() {
        let repo = Repo::new();
        repo.commit_file("tracked.txt", b"base");
        repo.file("Duplicate.txt", b"upper");
        repo.file("duplicate.txt", b"lower");
        let spellings = fs::read_dir(&repo.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_lowercase())
            .filter(|name| name == "duplicate.txt")
            .count();
        if spellings < 2 {
            assert_eq!(
                fs::read(repo.root.join("Duplicate.txt")).unwrap(),
                b"lower",
                "case-insensitive filesystem should alias the two path spellings"
            );
            return;
        }

        let error = capture(&options(&repo.root, "WORKTREE", true)).unwrap_err();
        assert!(error.to_string().contains("case-colliding"));
    }

    #[test]
    fn git_path_parser_rejects_non_utf8_paths() {
        assert!(super::utf8_path(b"valid/path").is_ok());
        assert!(super::utf8_path(b"invalid-\xff-path").is_err());
    }

    #[test]
    fn capture_enforces_per_file_snapshot_and_file_count_limits() {
        let repo = Repo::new();
        repo.commit_file("tracked.txt", b"base");
        repo.file("oversized.bin", &vec![b'x'; MAX_FILE_BYTES + 1]);
        assert!(capture(&options(&repo.root, "WORKTREE", true)).is_err());

        let mut body = CaptureBody {
            commit: String::new(),
            files: Vec::new(),
            objects: BTreeMap::new(),
        };
        let mut paths = HashSet::new();
        let mut total = MAX_SNAPSHOT_BYTES;
        assert!(add_file(
            &mut body,
            &mut paths,
            &mut total,
            "overflow.bin",
            0o644,
            vec![b'x']
        )
        .is_err());

        let mut collision_body = CaptureBody {
            commit: String::new(),
            files: Vec::new(),
            objects: BTreeMap::new(),
        };
        let mut collision_paths = HashSet::new();
        let mut collision_total = 0;
        add_file(
            &mut collision_body,
            &mut collision_paths,
            &mut collision_total,
            "Case.txt",
            0o644,
            b"first".to_vec(),
        )
        .unwrap();
        assert!(add_file(
            &mut collision_body,
            &mut collision_paths,
            &mut collision_total,
            "case.txt",
            0o644,
            b"second".to_vec()
        )
        .is_err());

        body.files = (0..MAX_FILES)
            .map(|index| SnapshotFile {
                path: format!("file-{index}"),
                mode: 0o644,
                sha256: String::new(),
                size: 0,
            })
            .collect();
        total = 0;
        assert!(add_file(
            &mut body,
            &mut paths,
            &mut total,
            "last.bin",
            0o644,
            Vec::new()
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn capture_into_state_persists_record_and_verified_blobs() {
        let repo = Repo::new();
        repo.commit_file("data.txt", b"persist me");
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");

        let snapshot = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture into state");

        let saved = state
            .records()
            .get_typed::<super::Snapshot>("snapshot", &snapshot.id)
            .expect("read stored snapshot");
        assert_eq!(saved, snapshot);
        assert_eq!(
            state.read_blob(&snapshot.files[0].sha256).unwrap(),
            b"persist me"
        );
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_materializes_exact_bytes_modes_and_rejects_modified_inputs() {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = Repo::new();
        repo.commit_file("nested/run.sh", b"#!/bin/sh\necho frozen\n");
        repo.git(&["update-index", "--chmod=+x", "nested/run.sh"]);
        repo.git(&["commit", "-q", "-m", "executable fixture"]);
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let snapshot = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture snapshot");

        validate_snapshot(&snapshot).expect("validate captured snapshot");
        let destination = repo.container.join("materialized");
        materialize(&state, &snapshot, &destination).expect("materialize snapshot");
        assert_eq!(
            fs::read(destination.join("nested/run.sh")).unwrap(),
            b"#!/bin/sh\necho frozen\n"
        );
        assert_eq!(
            fs::metadata(destination.join("nested/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        inputs_unchanged(&snapshot, &destination).expect("verify unchanged inputs");

        fs::write(destination.join("nested/run.sh"), b"changed").unwrap();
        assert!(inputs_unchanged(&snapshot, &destination).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn materialization_rejects_existing_files_without_replacing_them() {
        let repo = Repo::new();
        repo.commit_file("nested/value.txt", b"frozen bytes");
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let snapshot = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture snapshot");
        let destination = repo.container.join("materialized");
        fs::create_dir_all(destination.join("nested")).unwrap();
        fs::write(destination.join("nested/value.txt"), b"preserve me").unwrap();

        assert!(materialize(&state, &snapshot, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("nested/value.txt")).unwrap(),
            b"preserve me"
        );
    }

    #[cfg(unix)]
    #[test]
    fn materialization_checks_all_objects_before_creating_destination() {
        let repo = Repo::new();
        repo.commit_file("value.txt", b"snapshot bytes");
        let snapshot = capture(&options(&repo.root, "HEAD", false))
            .expect("capture snapshot")
            .snapshot;
        let state_path = repo.container.join("empty-state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open empty state");
        let destination = repo.container.join("should-not-exist");

        assert!(materialize(&state, &snapshot, &destination).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn snapshot_validation_rejects_identity_mode_digest_and_tree_conflicts() {
        let repo = Repo::new();
        repo.commit_file("dir/value.txt", b"snapshot bytes");
        let captured = capture(&options(&repo.root, "HEAD", false)).expect("capture snapshot");
        validate_snapshot(&captured.snapshot).expect("valid captured snapshot");

        let mut bad_identity = captured.snapshot.clone();
        bad_identity.id.push('x');
        assert!(validate_snapshot(&bad_identity).is_err());

        let mut bad_mode = captured.snapshot.clone();
        bad_mode.files[0].mode = 0o600;
        bad_mode.id = super::snapshot_id(&bad_mode.repository, &bad_mode.files).unwrap();
        assert!(validate_snapshot(&bad_mode).is_err());

        let mut bad_digest = captured.snapshot.clone();
        bad_digest.files[0].sha256 = "z".repeat(64);
        bad_digest.id = super::snapshot_id(&bad_digest.repository, &bad_digest.files).unwrap();
        assert!(validate_snapshot(&bad_digest).is_err());

        let mut conflict = captured.snapshot.clone();
        conflict.files.push(SnapshotFile {
            path: "dir".to_owned(),
            mode: 0o644,
            sha256: Sha256Digest::of(b"file").to_hex(),
            size: 4,
        });
        conflict
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        conflict.id = super::snapshot_id(&conflict.repository, &conflict.files).unwrap();
        assert!(validate_snapshot(&conflict).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn binary_diff_from_retained_snapshots_applies_to_exact_candidate_identity() {
        let repo = Repo::new();
        repo.file("a.txt", b"old\n");
        repo.file("drop.txt", b"remove\n");
        repo.file("space name.txt", b"before\n");
        repo.git(&["add", "--", "a.txt", "drop.txt", "space name.txt"]);
        repo.git(&["commit", "-q", "-m", "base"]);
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture base");

        repo.file("a.txt", b"new\n");
        repo.file("space name.txt", b"after\n");
        repo.file("binary.bin", b"\0\x01\x02");
        fs::remove_file(repo.root.join("drop.txt")).unwrap();
        repo.git(&["add", "--all"]);
        repo.git(&["commit", "-q", "-m", "candidate"]);
        let candidate = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture candidate");

        let patch = diff(&state, &base, &candidate).expect("generate retained-byte patch");
        assert!(String::from_utf8_lossy(&patch).contains("diff --git a/a.txt b/a.txt"));
        repo.git(&["checkout", "--detach", &base.commit]);
        let patch_file = repo.container.join("candidate.patch");
        fs::write(&patch_file, &patch).unwrap();
        repo.git(&["apply", "--check", patch_file.to_str().unwrap()]);
        repo.git(&["apply", patch_file.to_str().unwrap()]);
        let applied =
            capture(&options(&repo.root, "WORKTREE", true)).expect("capture applied patch");
        assert_eq!(applied.snapshot.id, candidate.id);
    }

    #[test]
    fn candidate_comparison_sorts_paths_and_reports_policy_and_test_deletions() {
        let repo = Repo::new();
        repo.file("tests/a_test.go", b"test");
        repo.file("value", b"base");
        repo.git(&["add", "--", "tests/a_test.go", "value"]);
        repo.git(&["commit", "-q", "-m", "base"]);
        let base = capture(&options(&repo.root, "HEAD", false))
            .expect("capture base")
            .snapshot;
        fs::remove_file(repo.root.join("tests/a_test.go")).unwrap();
        repo.file(".github/workflows/ci.yml", b"changed");
        repo.file(".rover/config.json", b"{}");
        let candidate = capture(&options(&repo.root, "WORKTREE", true))
            .expect("capture candidate")
            .snapshot;

        let inspection = compare_snapshots(&base, &candidate).expect("compare candidates");
        assert_eq!(
            inspection
                .changes
                .iter()
                .map(|change| change.path.as_str())
                .collect::<Vec<_>>(),
            [
                ".github/workflows/ci.yml",
                ".rover/config.json",
                "tests/a_test.go"
            ]
        );
        assert_eq!(inspection.findings.len(), 3);
        assert_eq!(
            inspection.findings[0].rule,
            "verification_configuration_changed"
        );
        assert_eq!(inspection.findings[2].rule, "test_file_deleted");
    }

    #[cfg(unix)]
    #[test]
    fn worktree_lifecycle_lists_inspects_and_removes_only_clean_worktrees() {
        let repo = Repo::new();
        repo.commit_file("src/value.txt", b"base bytes");
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture base");
        let destination = state
            .task_workspace_path("task-worktree-test")
            .expect("prepare private task worktree root");

        assert!(remove_worktree(&repo.root, &repo.root).is_err());
        add_worktree(&state, &base, &destination).expect("add detached worktree");
        assert!(add_worktree(&state, &base, &destination).is_err());
        let entries = list_worktrees(&repo.root).expect("list worktrees");
        let canonical_destination = fs::canonicalize(&destination).unwrap();
        let canonical_destination = canonical_destination.to_string_lossy();
        let entry = entries
            .iter()
            .find(|entry| entry.path == canonical_destination)
            .unwrap_or_else(|| {
                panic!(
                    "new worktree is not listed: destination={destination:?}, entries={entries:?}"
                )
            });
        assert!(entry.detached);
        assert_eq!(entry.head.as_deref(), Some(base.commit.as_str()));
        assert_eq!(
            fs::read(destination.join("src/value.txt")).unwrap(),
            b"base bytes"
        );

        fs::write(destination.join("src/value.txt"), b"changed bytes").unwrap();
        let inspection = inspect_worktree(&state, &base, &destination).expect("inspect worktree");
        assert!(inspection.dirty);
        assert_eq!(inspection.changes[0].path, "src/value.txt");
        assert_eq!(inspection.snapshot.consistency, WORKTREE_CONSISTENCY);
        assert!(remove_worktree(&repo.root, &destination).is_err());
        assert!(destination.join("src/value.txt").exists());

        fs::write(destination.join("src/value.txt"), b"base bytes").unwrap();
        let clean = inspect_worktree(&state, &base, &destination).expect("inspect clean worktree");
        assert!(!clean.dirty);
        remove_worktree(&repo.root, &destination).expect("remove clean worktree");
        assert!(!destination.exists());
        assert!(list_worktrees(&repo.root)
            .unwrap()
            .iter()
            .all(|entry| entry.path != canonical_destination));
    }

    #[cfg(unix)]
    #[test]
    fn worktree_create_does_not_run_smudge_filters_and_remove_preserves_ignored_data() {
        let repo = Repo::new();
        repo.file(".gitattributes", b"data.txt filter=probe\n");
        repo.file(".gitignore", b"ignored.txt\n");
        repo.file("data.txt", b"frozen bytes");
        repo.git(&["add", "--", ".gitattributes", ".gitignore", "data.txt"]);
        let marker = repo.container.join("filter-ran");
        repo.git(&[
            "config",
            "filter.probe.smudge",
            &format!("touch {} && cat", marker.display()),
        ]);
        repo.git(&["commit", "-q", "-m", "filtered fixture"]);
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture base");
        let destination = repo.container.join("filter-worktree");

        add_worktree(&state, &base, &destination).expect("add filter-free worktree");
        assert_eq!(
            fs::read(destination.join("data.txt")).unwrap(),
            b"frozen bytes"
        );
        assert!(!marker.exists());
        fs::write(
            destination.join("ignored.txt"),
            b"preserve ignored user data",
        )
        .unwrap();
        assert!(remove_worktree(&repo.root, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("ignored.txt")).unwrap(),
            b"preserve ignored user data"
        );
        fs::remove_file(destination.join("ignored.txt")).unwrap();
        remove_worktree(&repo.root, &destination).expect("remove clean worktree");
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_worktree_admission_is_unique_and_cleanup_is_idempotent() {
        let repo = Repo::new();
        repo.commit_file("value.txt", b"frozen base");
        let state_path = repo.container.join("state");
        let state =
            Arc::new(rover_store::files::StateRoot::open(&state_path).expect("open state root"));
        let base = Arc::new(
            super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
                .expect("capture base"),
        );
        let destination = state
            .task_workspace_path("task-admission-race")
            .expect("prepare worktree parent");
        let barrier = Arc::new(Barrier::new(3));
        let workers = [0, 1]
            .into_iter()
            .map(|_| {
                let state = Arc::clone(&state);
                let base = Arc::clone(&base);
                let destination = destination.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    add_worktree(&state, &base, &destination)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let admitted = workers
            .into_iter()
            .map(|worker| worker.join().expect("admission worker panicked"))
            .collect::<Vec<_>>();
        assert_eq!(admitted.iter().filter(|result| result.is_ok()).count(), 1);
        let canonical_destination = fs::canonicalize(&destination).unwrap();
        let canonical_destination = canonical_destination.to_string_lossy().into_owned();
        assert_eq!(
            list_worktrees(&repo.root)
                .unwrap()
                .iter()
                .filter(|entry| entry.path == canonical_destination)
                .count(),
            1
        );
        assert_eq!(
            fs::read(destination.join("value.txt")).unwrap(),
            b"frozen base"
        );

        let barrier = Arc::new(Barrier::new(3));
        let repo_root = repo.root.clone();
        let workers = [0, 1]
            .into_iter()
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let repo_root = repo_root.clone();
                let destination = destination.clone();
                thread::spawn(move || {
                    barrier.wait();
                    remove_worktree(&repo_root, &destination)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let removed = workers
            .into_iter()
            .map(|worker| worker.join().expect("cleanup worker panicked"))
            .collect::<Vec<_>>();
        assert!(removed.iter().any(Result::is_ok));
        assert!(!destination.exists());
        assert!(list_worktrees(&repo.root)
            .unwrap()
            .iter()
            .all(|entry| entry.path != canonical_destination));
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_worktree_capture_never_returns_bytes_with_a_wrong_digest() {
        let repo = Repo::new();
        let first = vec![b'A'; 4096];
        let second = vec![b'B'; 4096];
        repo.commit_file("value.bin", &first);
        let writer = repo.root.join(".value-swap");
        let repository = repo.root.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(2));
        let writer_stop = Arc::clone(&stop);
        let writer_barrier = Arc::clone(&barrier);
        let writer_first = first.clone();
        let writer_second = second.clone();
        let thread = thread::spawn(move || {
            writer_barrier.wait();
            let mut use_second = true;
            while !writer_stop.load(Ordering::Acquire) {
                let bytes = if use_second {
                    &writer_second
                } else {
                    &writer_first
                };
                fs::write(&writer, bytes).expect("write atomic replacement file");
                fs::rename(&writer, repository.join("value.bin"))
                    .expect("replace tracked input atomically");
                use_second = !use_second;
                thread::sleep(Duration::from_millis(3));
            }
        });
        barrier.wait();
        let mut successful_captures = 0;
        for _ in 0..16 {
            if let Ok(captured) = capture(&options(&repo.root, "WORKTREE", false)) {
                successful_captures += 1;
                validate_snapshot(&captured.snapshot).expect("captured manifest validates");
                for file in &captured.snapshot.files {
                    let bytes = captured
                        .objects
                        .get(&file.sha256)
                        .expect("captured object exists");
                    assert_eq!(i64::try_from(bytes.len()).unwrap(), file.size);
                    assert_eq!(Sha256Digest::of(bytes).to_hex(), file.sha256);
                    assert!(bytes == &first || bytes == &second);
                }
            }
        }
        stop.store(true, Ordering::Release);
        thread.join().expect("workspace writer panicked");
        assert!(
            successful_captures > 0,
            "capture should succeed during writer activity"
        );
    }

    #[test]
    fn porcelain_worktree_parser_rejects_unknown_or_invalid_metadata() {
        let parsed = super::parse_worktree_list(
            b"worktree /tmp/main\0HEAD 0123456789012345678901234567890123456789\0branch refs/heads/main\0\0worktree /tmp/detached\0HEAD abcdefabcdefabcdefabcdefabcdefabcdefabcd\0detached\0locked keep\0\0",
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].locked.as_deref(), Some("keep"));
        assert!(super::parse_worktree_list(b"worktree /tmp/repo\0new-field value\0\0").is_err());
        assert!(super::parse_worktree_list(b"worktree /tmp/repo\0HEAD invalid\0\0").is_err());
    }

    #[test]
    fn base_ref_resolution_and_branch_reservations_are_repository_scoped() {
        let repo = Repo::new();
        repo.commit_file("value.txt", b"base");
        let commit = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["branch", "topic/one"]);
        assert_eq!(resolve_reference(&repo.root, "topic/one").unwrap(), commit);
        assert!(resolve_reference(&repo.root, "--help").is_err());
        assert!(resolve_reference(&repo.root, "HEAD^{tree}").is_err());

        let branch = branch_reservation_resource(&repo.root, "topic/one").unwrap();
        rover_core::Id::parse(branch.clone()).expect("branch resource is a valid Rover ID");
        assert_eq!(
            branch,
            branch_reservation_resource(&repo.root, "topic/one").unwrap()
        );
        assert_ne!(
            branch,
            branch_reservation_resource(&repo.root, "topic/two").unwrap()
        );
        assert!(branch_reservation_resource(&repo.root, "bad..name").is_err());

        let other = Repo::new();
        other.commit_file("value.txt", b"base");
        assert_ne!(
            branch,
            branch_reservation_resource(&other.root, "topic/one").unwrap()
        );

        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let resources = vec![branch];
        state
            .records()
            .acquire_resources("task-owner-a", &resources, 4)
            .expect("reserve branch resource");
        assert!(state
            .records()
            .acquire_resources("task-owner-b", &resources, 4)
            .is_err());
        state
            .records()
            .release_resources("task-owner-a")
            .expect("release branch reservation");
        state
            .records()
            .acquire_resources("task-owner-b", &resources, 4)
            .expect("branch becomes available after owner release");
    }

    #[cfg(unix)]
    #[test]
    fn child_snapshot_replaces_files_without_moving_exact_base_commit() {
        let repo = Repo::new();
        repo.commit_file("src/value.txt", b"base bytes");
        let base_commit = repo.git(&["rev-parse", "HEAD"]);
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture base");

        repo.file("src/value.txt", b"initial snapshot bytes");
        repo.git(&["add", "--", "src/value.txt"]);
        repo.git(&["commit", "-q", "-m", "initial snapshot"]);
        let initial = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture initial snapshot");
        let destination = state
            .task_workspace_path("task-child-test")
            .expect("prepare task workspace");
        add_worktree(&state, &base, &destination).expect("add exact-base worktree");

        replace_worktree_snapshot(&state, &base, &initial, &destination)
            .expect("replace base files with initial snapshot");
        assert_eq!(
            fs::read(destination.join("src/value.txt")).unwrap(),
            b"initial snapshot bytes"
        );
        assert_eq!(
            repo.git(&["-C", destination.to_str().unwrap(), "rev-parse", "HEAD"]),
            base_commit
        );
        let mut child_options = options(&destination, "WORKTREE", true);
        child_options.state_root = state.path().to_path_buf();
        let observed = capture(&child_options).expect("capture child");
        assert_eq!(observed.snapshot.commit, base_commit);
        assert_eq!(
            observed
                .objects
                .get(&observed.snapshot.files[0].sha256)
                .unwrap(),
            b"initial snapshot bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn child_snapshot_replacement_refuses_dirty_base_without_changing_it() {
        let repo = Repo::new();
        repo.commit_file("src/value.txt", b"base bytes");
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture base");
        repo.file("src/value.txt", b"new snapshot");
        repo.git(&["add", "--", "src/value.txt"]);
        repo.git(&["commit", "-q", "-m", "initial"]);
        let initial = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture initial");
        let destination = state
            .task_workspace_path("task-dirty-test")
            .expect("prepare task workspace");
        add_worktree(&state, &base, &destination).expect("add worktree");
        fs::write(destination.join("src/value.txt"), b"concurrent edit").unwrap();

        assert!(replace_worktree_snapshot(&state, &base, &initial, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("src/value.txt")).unwrap(),
            b"concurrent edit"
        );
    }

    #[test]
    fn replacement_layout_conflicts_fail_before_any_filesystem_changes() {
        let base = [SnapshotFile {
            path: "tree/child.txt".to_owned(),
            mode: 0o644,
            sha256: Sha256Digest::of(b"child").to_hex(),
            size: 5,
        }];
        let initial = [SnapshotFile {
            path: "tree".to_owned(),
            mode: 0o644,
            sha256: Sha256Digest::of(b"file").to_hex(),
            size: 4,
        }];
        assert!(super::validate_replacement_layout(&base, &initial).is_err());
        assert!(super::validate_replacement_layout(&initial, &base).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn exact_base_overlay_and_merge_accept_disjoint_edits_and_reject_conflicts() {
        let repo = Repo::new();
        repo.file("a.txt", b"a0");
        repo.file("b.txt", b"b0");
        repo.file("gone.txt", b"gone");
        repo.git(&["add", "--", "a.txt", "b.txt", "gone.txt"]);
        repo.git(&["commit", "-q", "-m", "base"]);
        let state_path = repo.container.join("state");
        let state = rover_store::files::StateRoot::open(&state_path).expect("open state root");
        let base = super::capture_into_state(&options(&repo.root, "HEAD", false), &state)
            .expect("capture merge base");
        let put = |path: &str, bytes: &[u8], mut files: Vec<SnapshotFile>| {
            let digest = state.put_blob(bytes).unwrap().to_hex();
            if let Some(file) = files.iter_mut().find(|file| file.path == path) {
                file.sha256 = digest;
                file.size = i64::try_from(bytes.len()).unwrap();
            } else {
                files.push(SnapshotFile {
                    path: path.to_owned(),
                    mode: 0o644,
                    sha256: digest,
                    size: i64::try_from(bytes.len()).unwrap(),
                });
            }
            files
        };

        let mut first_files = put("a.txt", b"a1", base.files.clone());
        first_files.retain(|file| file.path != "gone.txt");
        let first = compose_snapshot(&state, &base, first_files, "test:first")
            .expect("compose first candidate");
        let second_files = put("b.txt", b"b1", base.files.clone());
        let second_files = put("new.txt", b"new", second_files);
        let second = compose_snapshot(&state, &base, second_files, "test:second")
            .expect("compose second candidate");

        let merged = merge_snapshots(&state, &base, &[first.clone(), second.clone()])
            .expect("merge disjoint candidates");
        let verification = verify_retained_snapshot(&state, &merged.id)
            .expect("verify persisted integrated snapshot");
        assert_eq!(verification.verified_files, 3);
        assert_eq!(verification.verified_bytes, 7);
        let merged_paths = merged
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(merged_paths, ["a.txt", "b.txt", "new.txt"]);
        for (path, expected) in [
            ("a.txt", b"a1".as_slice()),
            ("b.txt", b"b1"),
            ("new.txt", b"new"),
        ] {
            let file = merged.files.iter().find(|file| file.path == path).unwrap();
            assert_eq!(state.read_blob(&file.sha256).unwrap(), expected);
        }

        let overlay = overlay_snapshot(&state, &base, &first, &["a.txt".to_owned()])
            .expect("overlay selected path");
        assert!(overlay.files.iter().any(|file| file.path == "gone.txt"));
        assert_eq!(compare_snapshots(&base, &overlay).unwrap().changes.len(), 1);
        assert!(overlay_snapshot(&state, &base, &first, &[]).is_err());
        assert!(overlay_snapshot(
            &state,
            &base,
            &first,
            &["a.txt".to_owned(), "a.txt".to_owned()]
        )
        .is_err());

        let identical_files = put("a.txt", b"a1", base.files.clone());
        let identical = compose_snapshot(&state, &base, identical_files, "test:identical")
            .expect("compose identical edit");
        let identical_merge = merge_snapshots(&state, &base, &[first.clone(), identical])
            .expect("identical edits are compatible");
        assert!(identical_merge
            .files
            .iter()
            .all(|file| file.path != "gone.txt"));

        let edit_deleted_files = put("gone.txt", b"updated", base.files.clone());
        let edit_deleted = compose_snapshot(&state, &base, edit_deleted_files, "test:edit-deleted")
            .expect("compose edit of concurrently deleted file");
        assert!(merge_snapshots(&state, &base, &[first.clone(), edit_deleted]).is_err());

        let conflict_files = put("a.txt", b"other edit", base.files.clone());
        let conflict = compose_snapshot(&state, &base, conflict_files, "test:conflict")
            .expect("compose conflicting candidate");
        assert!(merge_snapshots(&state, &base, &[first, conflict]).is_err());

        let mut other_base = second.clone();
        other_base.commit = "0123456789012345678901234567890123456789".to_owned();
        assert!(merge_snapshots(&state, &base, &[other_base]).is_err());

        let missing = merged
            .files
            .iter()
            .find(|file| file.path == "new.txt")
            .unwrap();
        fs::remove_file(state_path.join("objects").join(&missing.sha256)).unwrap();
        assert!(verify_retained_snapshot(&state, &merged.id).is_err());
    }
}
