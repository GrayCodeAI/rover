//! Rooted repository file browsing and editing for the Rust CLI/TUI.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use rover_core::Sha256Digest;
use rover_store::files::{SafeDir, SafeEntryKind};

use crate::git;

/// Maximum entries returned by one directory listing or quick-open scan.
pub const MAX_BROWSER_ENTRIES: usize = 10_000;
/// Maximum file preview bytes returned to the TUI.
pub const MAX_FILE_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_BROWSER_DEPTH: usize = 48;

/// Filesystem kind presented by the read-only browser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserEntryKind {
    /// Regular file, eligible for a bounded text preview.
    File,
    /// Directory, eligible for navigation.
    Directory,
    /// Symlink, displayed but never traversed or opened.
    Symlink,
    /// Special filesystem object, displayed but never opened.
    Other,
}

/// Git worktree status used for file-tree coloring and labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserGitStatus {
    /// Added in the index or worktree.
    Added,
    /// Modified or type-changed.
    Modified,
    /// Deleted from the index or worktree.
    Deleted,
    /// Renamed or copied according to porcelain status.
    Renamed,
    /// Untracked by Git.
    Untracked,
    /// Conflict stages are present.
    Conflict,
}

/// One repository-relative path in a browser result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserEntry {
    /// Slash-separated repository-relative path.
    pub path: String,
    /// No-follow filesystem kind.
    pub kind: BrowserEntryKind,
    /// Git worktree status; directories aggregate descendant status.
    pub git_status: Option<BrowserGitStatus>,
}

/// Bounded direct-child file-tree listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserListing {
    /// Sorted entries; directories sort before non-directories.
    pub entries: Vec<BrowserEntry>,
    /// More eligible entries existed than the configured result limit.
    pub truncated: bool,
    /// Git status could not be read or parsed; filesystem browsing remains available.
    pub git_error: Option<String>,
}

/// Read one directory in a canonical repository without following links.
///
/// # Errors
///
/// Returns an error for an invalid project path, invalid relative directory,
/// unsafe directory entry, or filesystem read failure.
pub fn browse_directory(
    repository: &Path,
    relative_directory: &str,
    include_hidden: bool,
) -> io::Result<BrowserListing> {
    let root = open_repository(repository)?;
    let (status_map, git_error) = load_git_status(repository);
    let (raw_entries, truncated) = root.list_directory_path(
        relative_directory,
        &enumeration_path(repository, relative_directory)?,
        include_hidden,
        MAX_BROWSER_ENTRIES,
    )?;
    let mut entries = raw_entries
        .into_iter()
        .map(|entry| {
            let path = join_relative(relative_directory, &entry.name);
            let git_status = status_for_path(&status_map, &path, entry.kind);
            BrowserEntry {
                path,
                kind: browser_kind(entry.kind),
                git_status,
            }
        })
        .collect::<Vec<_>>();
    let existing = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let prefix = if relative_directory.is_empty() {
        String::new()
    } else {
        format!("{relative_directory}/")
    };
    let mut truncated = truncated;
    for (path, status) in &status_map {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if *status != BrowserGitStatus::Deleted
            || name.is_empty()
            || name.contains('/')
            || (!include_hidden && name.starts_with('.'))
            || existing.contains(path)
        {
            continue;
        }
        if entries.len() >= MAX_BROWSER_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(BrowserEntry {
            path: path.clone(),
            kind: BrowserEntryKind::File,
            git_status: Some(BrowserGitStatus::Deleted),
        });
    }
    entries.sort_by(|left, right| {
        (left.kind != BrowserEntryKind::Directory)
            .cmp(&(right.kind != BrowserEntryKind::Directory))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(BrowserListing {
        entries,
        truncated,
        git_error,
    })
}

/// Enumerate repository files for hidden-file quick-open, excluding links,
/// special files, `.git`, and trees deeper than the explicit bound.
///
/// # Errors
///
/// Returns an error for an invalid project path, unsafe directory entry, or
/// filesystem enumeration failure.
pub fn quick_open_files(repository: &Path) -> io::Result<BrowserListing> {
    let root = open_repository(repository)?;
    let (status_map, git_error) = load_git_status(repository);
    let mut pending = vec![String::new()];
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        if directory.split('/').count() > MAX_BROWSER_DEPTH {
            truncated = true;
            continue;
        }
        let enumeration = enumeration_path(repository, &directory)?;
        let remaining = MAX_BROWSER_ENTRIES.saturating_sub(visited);
        if remaining == 0 {
            truncated = true;
            break;
        }
        let (children, listing_truncated) =
            root.list_directory_path(&directory, &enumeration, true, remaining)?;
        truncated |= listing_truncated;
        for child in children {
            visited = visited.saturating_add(1);
            let path = join_relative(&directory, &child.name);
            match child.kind {
                SafeEntryKind::Directory => pending.push(path),
                SafeEntryKind::File => entries.push(BrowserEntry {
                    git_status: status_for_path(&status_map, &path, child.kind),
                    path,
                    kind: BrowserEntryKind::File,
                }),
                SafeEntryKind::Symlink | SafeEntryKind::Other => {}
            }
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(BrowserListing {
        entries,
        truncated,
        git_error,
    })
}

/// Read at most 256 KiB from one regular repository file without following
/// symlinks. The returned boolean indicates omitted trailing bytes.
///
/// # Errors
///
/// Returns an error for an invalid project path, unsafe relative path,
/// symlink, non-regular file, or filesystem read failure.
pub fn read_repository_file(repository: &Path, relative_path: &str) -> io::Result<(Vec<u8>, bool)> {
    open_repository(repository)?.read_regular_file_path(relative_path, MAX_FILE_PREVIEW_BYTES)
}

/// Read one regular text-edit candidate up to Rover's 8 MiB source file cap.
///
/// # Errors
///
/// Returns an error for an unsafe path, symlink, non-regular file, oversized
/// file, or filesystem failure.
pub fn read_repository_file_for_edit(
    repository: &Path,
    relative_path: &str,
) -> io::Result<Vec<u8>> {
    let (bytes, truncated) = open_repository(repository)?
        .read_regular_file_path(relative_path, crate::MAX_FILE_BYTES)?;
    if truncated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds the 8 MiB edit limit",
        ));
    }
    Ok(bytes)
}

/// Replace one existing regular repository file if its bytes still match the
/// digest observed when editing began. The replacement is rooted, no-follow,
/// atomic, and synced before this function returns.
///
/// # Errors
///
/// Returns an error for unsafe paths, oversized files, symlinks or special
/// entries, stale content, or filesystem failures.
pub fn save_repository_file(
    repository: &Path,
    relative_path: &str,
    expected_sha256: &str,
    bytes: &[u8],
) -> io::Result<()> {
    validate_relative_path(relative_path)?;
    if bytes.len() > crate::MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "edited file exceeds the 8 MiB source file limit",
        ));
    }
    let root = open_repository(repository)?;
    let (current, truncated) = root.read_regular_file_path(relative_path, crate::MAX_FILE_BYTES)?;
    if truncated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "current file exceeds the source file limit",
        ));
    }
    if Sha256Digest::of(&current).to_hex() != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "file changed since editing began; reload before saving",
        ));
    }
    root.atomic_replace_regular_file_path(relative_path, bytes, crate::MAX_FILE_BYTES)
}

fn open_repository(repository: &Path) -> io::Result<SafeDir> {
    let canonical = std::fs::canonicalize(repository)?;
    if !canonical.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not a directory",
        ));
    }
    SafeDir::open_directory(canonical)
}

fn enumeration_path(repository: &Path, relative_directory: &str) -> io::Result<PathBuf> {
    if relative_directory.is_empty() {
        return Ok(repository.to_path_buf());
    }
    validate_relative_path(relative_directory)?;
    Ok(repository.join(relative_directory))
}

fn validate_relative_path(path: &str) -> io::Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|part| {
            part.is_empty() || matches!(part, "." | "..") || part.eq_ignore_ascii_case(".git")
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository-relative path is unsafe",
        ));
    }
    Ok(())
}

fn join_relative(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}

fn browser_kind(kind: SafeEntryKind) -> BrowserEntryKind {
    match kind {
        SafeEntryKind::File => BrowserEntryKind::File,
        SafeEntryKind::Directory => BrowserEntryKind::Directory,
        SafeEntryKind::Symlink => BrowserEntryKind::Symlink,
        SafeEntryKind::Other => BrowserEntryKind::Other,
    }
}

fn load_git_status(repository: &Path) -> (BTreeMap<String, BrowserGitStatus>, Option<String>) {
    match git(
        repository,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--renames",
            "--",
        ],
    ) {
        Ok(bytes) => match parse_git_status(&bytes) {
            Ok(status) => (status, None),
            Err(error) => (BTreeMap::new(), Some(error.to_string())),
        },
        Err(error) => (BTreeMap::new(), Some(error.to_string())),
    }
}

fn parse_git_status(bytes: &[u8]) -> io::Result<BTreeMap<String, BrowserGitStatus>> {
    let mut statuses = BTreeMap::new();
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Git returned malformed porcelain status",
            ));
        }
        let path = std::str::from_utf8(&record[3..]).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Git path is not UTF-8 and cannot be shown by the TUI",
            )
        })?;
        validate_relative_path(path)?;
        let status = parse_status_code([record[0], record[1]]);
        statuses.insert(path.to_owned(), status);
        if (record[0] == b'R' || record[0] == b'C' || record[1] == b'R' || record[1] == b'C')
            && records.next().is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Git rename status omitted its source path",
            ));
        }
    }
    Ok(statuses)
}

fn parse_status_code(code: [u8; 2]) -> BrowserGitStatus {
    let [index, worktree] = code;
    if matches!(
        (index, worktree),
        (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D')
    ) {
        BrowserGitStatus::Conflict
    } else if index == b'?' && worktree == b'?' {
        BrowserGitStatus::Untracked
    } else if index == b'D' || worktree == b'D' {
        BrowserGitStatus::Deleted
    } else if index == b'A' || worktree == b'A' {
        BrowserGitStatus::Added
    } else if index == b'R' || worktree == b'R' || index == b'C' || worktree == b'C' {
        BrowserGitStatus::Renamed
    } else {
        BrowserGitStatus::Modified
    }
}

fn status_for_path(
    statuses: &BTreeMap<String, BrowserGitStatus>,
    path: &str,
    kind: SafeEntryKind,
) -> Option<BrowserGitStatus> {
    statuses.get(path).copied().or_else(|| {
        (kind == SafeEntryKind::Directory)
            .then(|| {
                statuses
                    .range(format!("{path}/")..)
                    .next()
                    .filter(|(candidate, _)| candidate.starts_with(&format!("{path}/")))
                    .map(|(_, status)| *status)
            })
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Repo(PathBuf);

    impl Repo {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rover-browser-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            git_fixture(&path, &["init", "-q"]);
            git_fixture(&path, &["config", "user.email", "rover@example.invalid"]);
            git_fixture(&path, &["config", "user.name", "Rover fixture"]);
            fs::create_dir(path.join("src")).unwrap();
            fs::write(path.join("src/main.rs"), "fn main() {}\n").unwrap();
            git_fixture(&path, &["add", "--", "src/main.rs"]);
            git_fixture(&path, &["commit", "-q", "-m", "fixture"]);
            Self(path)
        }

        fn write(&self, path: &str, bytes: &[u8]) {
            let full = self.0.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, bytes).unwrap();
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git_fixture(repository: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .args(["-C"])
            .arg(repository)
            .args(arguments)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn browser_lists_hidden_and_git_state_without_traversing_symlinks() {
        let repo = Repo::new();
        repo.write("src/main.rs", b"changed");
        repo.write("src/new.rs", b"new");
        repo.write("src/gone.rs", b"will be deleted");
        git_fixture(&repo.0, &["add", "--", "src/gone.rs"]);
        git_fixture(&repo.0, &["commit", "-q", "-m", "add deletion fixture"]);
        fs::remove_file(repo.0.join("src/gone.rs")).unwrap();
        repo.write(".hidden/settings.json", b"hidden");
        symlink(std::env::temp_dir(), repo.0.join("outside-link")).unwrap();

        let visible = browse_directory(&repo.0, "", false).unwrap();
        assert!(visible
            .entries
            .iter()
            .all(|entry| !entry.path.starts_with('.')));
        assert!(visible
            .entries
            .iter()
            .any(|entry| entry.path == "outside-link" && entry.kind == BrowserEntryKind::Symlink));
        let hidden = browse_directory(&repo.0, "", true).unwrap();
        assert!(hidden.entries.iter().any(|entry| entry.path == ".hidden"));
        let source = browse_directory(&repo.0, "src", false).unwrap();
        assert_eq!(
            source
                .entries
                .iter()
                .find(|entry| entry.path == "src/main.rs")
                .unwrap()
                .git_status,
            Some(BrowserGitStatus::Modified)
        );
        assert_eq!(
            source
                .entries
                .iter()
                .find(|entry| entry.path == "src/new.rs")
                .unwrap()
                .git_status,
            Some(BrowserGitStatus::Untracked)
        );
        assert_eq!(
            source
                .entries
                .iter()
                .find(|entry| entry.path == "src/gone.rs")
                .unwrap()
                .git_status,
            Some(BrowserGitStatus::Deleted)
        );
        assert!(browse_directory(&repo.0, "outside-link", true).is_err());
    }

    #[test]
    fn quick_open_includes_hidden_files_but_skips_git_and_links() {
        let repo = Repo::new();
        repo.write(".hidden/config.toml", b"setting=true");
        repo.write("nested/deeper/note.md", b"note");
        symlink(
            std::env::temp_dir().join("not-a-rover-file"),
            repo.0.join("alias"),
        )
        .unwrap();
        let listing = quick_open_files(&repo.0).unwrap();
        assert!(listing
            .entries
            .iter()
            .any(|entry| entry.path == ".hidden/config.toml"));
        assert!(listing
            .entries
            .iter()
            .any(|entry| entry.path == "nested/deeper/note.md"));
        assert!(listing
            .entries
            .iter()
            .all(|entry| !entry.path.starts_with(".git/")));
        assert!(listing.entries.iter().all(|entry| entry.path != "alias"));
    }

    #[test]
    fn bounded_preview_refuses_traversal_links_and_oversize_limits() {
        let repo = Repo::new();
        repo.write("preview.txt", b"hello preview");
        symlink(
            std::env::temp_dir().join("external-secret"),
            repo.0.join("link.txt"),
        )
        .unwrap();
        let (bytes, truncated) = read_repository_file(&repo.0, "preview.txt").unwrap();
        assert_eq!(bytes, b"hello preview");
        assert!(!truncated);
        repo.write("large.txt", &vec![b'x'; MAX_FILE_PREVIEW_BYTES + 9]);
        let (large, truncated) = read_repository_file(&repo.0, "large.txt").unwrap();
        assert_eq!(large.len(), MAX_FILE_PREVIEW_BYTES);
        assert!(truncated);
        assert!(read_repository_file(&repo.0, "../external-secret").is_err());
        assert!(read_repository_file(&repo.0, "link.txt").is_err());
    }

    #[test]
    fn save_checks_original_digest_and_atomically_replaces_regular_file() {
        let repo = Repo::new();
        let original = b"fn main() {}\n";
        let expected = Sha256Digest::of(original).to_hex();
        save_repository_file(&repo.0, "src/main.rs", &expected, b"fn main() { 1; }\n").unwrap();
        assert_eq!(
            fs::read(repo.0.join("src/main.rs")).unwrap(),
            b"fn main() { 1; }\n"
        );
        let stale = save_repository_file(&repo.0, "src/main.rs", &expected, b"stale");
        assert_eq!(stale.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert!(save_repository_file(&repo.0, "../escape", &expected, b"bad").is_err());
        symlink(std::env::temp_dir(), repo.0.join("linked.rs")).unwrap();
        assert!(save_repository_file(&repo.0, "linked.rs", &expected, b"bad").is_err());
    }

    #[test]
    fn porcelain_parser_handles_status_codes_and_rejects_invalid_paths() {
        let bytes = b" M src/main.rs\0?? notes.txt\0A  added.rs\0D  gone.rs\0UU conflict.rs\0R  renamed.rs\0old-name.rs\0";
        let statuses = parse_git_status(bytes).unwrap();
        assert_eq!(statuses["src/main.rs"], BrowserGitStatus::Modified);
        assert_eq!(statuses["notes.txt"], BrowserGitStatus::Untracked);
        assert_eq!(statuses["added.rs"], BrowserGitStatus::Added);
        assert_eq!(statuses["gone.rs"], BrowserGitStatus::Deleted);
        assert_eq!(statuses["conflict.rs"], BrowserGitStatus::Conflict);
        assert_eq!(statuses["renamed.rs"], BrowserGitStatus::Renamed);
        assert!(parse_git_status(b" M ../escape\0").is_err());
        assert!(parse_git_status(b"bad\0").is_err());
    }
}
