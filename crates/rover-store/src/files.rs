//! Unix directory-handle anchored file operations for Rover state.
//!
//! These APIs operate on one already-admitted directory and one basename per
//! call. They do not create or admit a Rover state root.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rover_core::Sha256Digest;
use rusqlite::{Connection, OpenFlags};
use rustix::fs::{
    fchmod, flock, fstat, fsync, linkat, mkdirat, open, openat, renameat, statat, unlinkat,
    AtFlags, FileType, FlockOperation, Mode, OFlags, RawMode,
};

use crate::{Store, StoreError};

const MAX_BLOB_BYTES: u64 = 16 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Entry kind returned by a descriptor-validated directory enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafeEntryKind {
    /// Regular file that can be opened through the bounded reader.
    File,
    /// Directory that can be entered without following a link.
    Directory,
    /// Symlink shown for awareness but never traversed or opened.
    Symlink,
    /// Device, socket, FIFO, or other unsupported filesystem object.
    Other,
}

/// One UTF-8 name and no-follow type from a safe directory listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeDirectoryEntry {
    /// One child basename relative to the listed directory.
    pub name: String,
    /// Type observed without following the entry.
    pub kind: SafeEntryKind,
}

/// A validated open directory. All later operations are relative to its file
/// descriptor, so renaming the path after opening does not redirect access.
pub struct SafeDir {
    descriptor: OwnedFd,
}

impl SafeDir {
    /// Open an existing directory without following a symlink at its final
    /// path component. Unlike [`SafeDir::open`], this does not impose private
    /// permission bits and is intended for user-selected workspace roots.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the path is a symlink or is not a directory.
    pub fn open_directory(path: impl AsRef<Path>) -> io::Result<Self> {
        let descriptor = open(
            path.as_ref(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        if !FileType::from_raw_mode(fstat(&descriptor)?.st_mode).is_dir() {
            return Err(invalid_data("path is not a directory"));
        }
        Ok(Self { descriptor })
    }

    /// List one directory beneath this handle and validate each returned name
    /// against the descriptor-opened directory without following symlinks.
    ///
    /// The empty path lists this directory. `enumeration_path` must name the
    /// same directory through its ordinary filesystem path; entries observed
    /// there are revalidated against this handle before they are returned.
    /// Names are UTF-8-only, `.git` is always omitted, and hidden names are
    /// included only when requested.
    /// The result is sorted by name. The boolean reports whether more entries
    /// existed than the requested limit.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe path, invalid limit, unreadable
    /// directory, or a non-UTF-8 entry name.
    pub fn list_directory_path(
        &self,
        relative_directory: &str,
        enumeration_path: &Path,
        include_hidden: bool,
        limit: usize,
    ) -> io::Result<(Vec<SafeDirectoryEntry>, bool)> {
        if !(1..=10_000).contains(&limit) {
            return Err(invalid_input("directory entry limit is outside 1..=10000"));
        }
        let directory = self.open_relative_directory(relative_directory)?;
        let iterator = fs::read_dir(enumeration_path)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        for entry in iterator {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid_data("directory entry name is not UTF-8"))?;
            if name == "."
                || name == ".."
                || name.eq_ignore_ascii_case(".git")
                || (!include_hidden && name.starts_with('.'))
            {
                continue;
            }
            validate_basename(&name)?;
            let metadata = match statat(&directory.descriptor, &name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(metadata) => metadata,
                Err(error) if error == rustix::io::Errno::NOENT => continue,
                Err(error) => return Err(error.into()),
            };
            let kind = FileType::from_raw_mode(metadata.st_mode);
            let kind = if kind.is_file() {
                SafeEntryKind::File
            } else if kind.is_dir() {
                SafeEntryKind::Directory
            } else if kind.is_symlink() {
                SafeEntryKind::Symlink
            } else {
                SafeEntryKind::Other
            };
            if entries.len() == limit {
                truncated = true;
                break;
            }
            entries.push(SafeDirectoryEntry { name, kind });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok((entries, truncated))
    }

    /// Read a regular file beneath this handle, rejecting symlinks and
    /// returning at most `max_bytes` plus a truncation flag.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe path, invalid bound, missing parent,
    /// symlink, non-regular file, or read failure.
    pub fn read_regular_file_path(
        &self,
        relative_file_path: &str,
        max_bytes: usize,
    ) -> io::Result<(Vec<u8>, bool)> {
        let max_bytes_u64 = u64::try_from(max_bytes)
            .map_err(|_| invalid_input("file read limit exceeds 16 MiB"))?;
        if max_bytes_u64 > MAX_BLOB_BYTES {
            return Err(invalid_input("file read limit exceeds 16 MiB"));
        }
        let components = relative_path_components(relative_file_path, false)?;
        let (filename, parents) = components
            .split_last()
            .ok_or_else(|| invalid_input("relative file path is empty"))?;
        let parent = self.open_relative_directory(&parents.join("/"))?;
        validate_basename(filename)?;
        let descriptor = openat(
            &parent.descriptor,
            *filename,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?;
        let file = File::from(descriptor);
        if !FileType::from_raw_mode(fstat(file.as_fd())?.st_mode).is_file() {
            return Err(invalid_data("file viewer only opens regular files"));
        }
        let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
        file.take(max_bytes_u64.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max_bytes;
        bytes.truncate(max_bytes);
        Ok((bytes, truncated))
    }

    /// Atomically replace a regular file beneath this handle without following
    /// any symlink in the path. Existing files keep their permission bits;
    /// new files are created with mode `0644`. The replacement is durable to
    /// the extent provided by syncing the file and containing directory.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe path, oversized content, a symlink or
    /// non-regular existing entry, or any filesystem operation failure.
    pub fn atomic_replace_regular_file_path(
        &self,
        relative_file_path: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> io::Result<()> {
        if bytes.len() > maximum_bytes
            || u64::try_from(maximum_bytes).map_or(true, |limit| limit > MAX_BLOB_BYTES)
        {
            return Err(invalid_input("replacement file exceeds its byte limit"));
        }
        let components = relative_path_components(relative_file_path, false)?;
        let (filename, parents) = components
            .split_last()
            .ok_or_else(|| invalid_input("relative file path is empty"))?;
        let parent = self.open_relative_directory(&parents.join("/"))?;
        validate_basename(filename)?;
        let mode = match statat(&parent.descriptor, *filename, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) if FileType::from_raw_mode(metadata.st_mode).is_file() => {
                metadata.st_mode & 0o777
            }
            Ok(_) => return Err(invalid_data("replacement target is not a regular file")),
            Err(error) if error == rustix::io::Errno::NOENT => 0o644,
            Err(error) => return Err(error.into()),
        };
        let (temp_name, mut file) = parent.create_temp("edit")?;
        let _cleanup = TempEntry::new(&parent, temp_name.clone());
        let mode = RawMode::try_from(mode)
            .map_err(|_| invalid_input("file mode does not fit platform mode type"))?;
        fchmod(file.as_fd(), Mode::from_raw_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        renameat(
            &parent.descriptor,
            &temp_name,
            &parent.descriptor,
            *filename,
        )?;
        fsync(&parent.descriptor)?;
        Ok(())
    }

    fn open_relative_directory(&self, relative_directory: &str) -> io::Result<Self> {
        let components = relative_path_components(relative_directory, true)?;
        let descriptor = self.descriptor.as_fd().try_clone_to_owned()?;
        let mut directory = SafeDir { descriptor };
        for component in components {
            directory = directory.open_subdirectory(component)?;
        }
        Ok(directory)
    }

    /// Open an existing directory without following a symlink in its final
    /// path component. The directory must be mode `0700`: owner access is
    /// required and group/other access is rejected.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the path is a symlink, is not a directory, or
    /// does not have the required private permissions.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let descriptor = open(
            path.as_ref(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let metadata = fstat(&descriptor)?;
        if !FileType::from_raw_mode(metadata.st_mode).is_dir() {
            return Err(invalid_data("state path is not a directory"));
        }
        if metadata.st_mode & 0o777 != 0o700 {
            return Err(invalid_data("state directory permissions must be 0700"));
        }
        Ok(Self { descriptor })
    }

    /// Open a child directory relative to this descriptor, creating it with
    /// mode `0700` if it does not exist. Existing symlinks and non-directories
    /// are refused; existing directory permissions are preserved.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the name is not one safe path component, the
    /// child is not a directory, or directory creation/open fails.
    pub fn ensure_subdirectory(&self, name: &str) -> io::Result<Self> {
        validate_basename(name)?;
        let descriptor = match openat(
            &self.descriptor,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(error) if error == rustix::io::Errno::NOENT => {
                match mkdirat(&self.descriptor, name, Mode::from(0o700)) {
                    Ok(()) => fsync(&self.descriptor)?,
                    Err(error) if error == rustix::io::Errno::EXIST => {}
                    Err(error) => return Err(error.into()),
                }
                openat(
                    &self.descriptor,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?
            }
            Err(error) => return Err(error.into()),
        };
        if !FileType::from_raw_mode(fstat(&descriptor)?.st_mode).is_dir() {
            return Err(invalid_data("child path is not a directory"));
        }
        Ok(Self { descriptor })
    }

    /// Remove one regular file beneath this directory by a slash-separated
    /// relative path. Every parent is opened without following symlinks and
    /// the leaf is unlinked relative to its parent descriptor.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unsafe paths, missing/non-directory parents,
    /// non-regular leaf entries, or failed unlink/sync operations.
    pub fn remove_regular_file_path(&self, relative_file_path: &str) -> io::Result<()> {
        if relative_file_path.is_empty() || relative_file_path.contains('\\') {
            return Err(invalid_input("relative file path is invalid"));
        }
        let components = relative_file_path.split('/').collect::<Vec<_>>();
        if components
            .iter()
            .any(|component| component.is_empty() || *component == "." || *component == "..")
        {
            return Err(invalid_input(
                "relative file path contains an unsafe component",
            ));
        }
        let descriptor = self.descriptor.as_fd().try_clone_to_owned()?;
        let mut parent = SafeDir { descriptor };
        for component in &components[..components.len() - 1] {
            parent = parent.open_subdirectory(component)?;
        }
        let name = components[components.len() - 1];
        validate_basename(name)?;
        let metadata = statat(&parent.descriptor, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if !FileType::from_raw_mode(metadata.st_mode).is_file() {
            return Err(invalid_data("removal target is not a regular file"));
        }
        unlinkat(&parent.descriptor, name, AtFlags::empty())?;
        fsync(&parent.descriptor)?;
        Ok(())
    }

    fn lock_exclusive_file(&self, name: &str) -> io::Result<File> {
        validate_basename(name)?;
        let descriptor = openat(
            &self.descriptor,
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from(0o600),
        )?;
        let file = File::from(descriptor);
        let metadata = fstat(file.as_fd())?;
        if !FileType::from_raw_mode(metadata.st_mode).is_file() {
            return Err(invalid_data(
                "worktree admission lock is not a regular file",
            ));
        }
        fchmod(file.as_fd(), Mode::from(0o600))?;
        flock(file.as_fd(), FlockOperation::LockExclusive)?;
        Ok(file)
    }

    fn open_subdirectory(&self, name: &str) -> io::Result<Self> {
        validate_basename(name)?;
        let descriptor = openat(
            &self.descriptor,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        if !FileType::from_raw_mode(fstat(&descriptor)?.st_mode).is_dir() {
            return Err(invalid_data("child path is not a directory"));
        }
        Ok(Self { descriptor })
    }

    /// Walk or create parent directories for a slash-separated relative file
    /// path and return the final parent directory descriptor. The last path
    /// component is intentionally left for a subsequent file operation.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the path is absolute, contains an unsafe
    /// component, or any parent cannot be opened as a directory.
    pub fn ensure_subdirectory_path(&self, relative_file_path: &str) -> io::Result<Self> {
        if relative_file_path.is_empty() || relative_file_path.contains('\\') {
            return Err(invalid_input("relative file path is invalid"));
        }
        let components = relative_file_path.split('/').collect::<Vec<_>>();
        if components
            .iter()
            .any(|component| component.is_empty() || *component == "." || *component == "..")
        {
            return Err(invalid_input(
                "relative file path contains an unsafe component",
            ));
        }
        let descriptor = self.descriptor.as_fd().try_clone_to_owned()?;
        let mut directory = SafeDir { descriptor };
        for component in &components[..components.len() - 1] {
            directory = directory.ensure_subdirectory(component)?;
        }
        Ok(directory)
    }

    /// Set permission bits on this already-open directory descriptor and sync
    /// the metadata update.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the mode is unsupported or the update/sync fails.
    pub fn set_permissions(&self, mode: u32) -> io::Result<()> {
        if mode & !0o777 != 0 {
            return Err(invalid_input("directory mode contains unsupported bits"));
        }
        let mode = RawMode::try_from(mode)
            .map_err(|_| invalid_input("directory mode does not fit platform mode type"))?;
        fchmod(&self.descriptor, Mode::from_raw_mode(mode))?;
        fsync(&self.descriptor)?;
        Ok(())
    }

    /// Create a new regular file relative to this directory descriptor. The
    /// operation is exclusive, never follows or replaces an existing entry,
    /// applies the exact requested mode, syncs file bytes, then syncs the
    /// directory entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for an invalid basename or mode, an existing
    /// destination entry, or a failed write/sync.
    pub fn create_new_file(&self, name: &str, bytes: &[u8], mode: u32) -> io::Result<()> {
        validate_basename(name)?;
        if mode & !0o777 != 0 {
            return Err(invalid_input(
                "file mode contains unsupported permission bits",
            ));
        }
        let raw_mode = RawMode::try_from(mode)
            .map_err(|_| invalid_input("file mode does not fit platform mode type"))?;
        let mut file = File::from(openat(
            &self.descriptor,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(raw_mode),
        )?);
        let result = (|| {
            fchmod(file.as_fd(), Mode::from_raw_mode(raw_mode))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fsync(&self.descriptor)?;
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = unlinkat(&self.descriptor, name, AtFlags::empty());
        }
        result
    }

    /// Create a new mode-controlled regular file relative to this directory
    /// descriptor and return its open handle for bounded streaming writes.
    /// The directory entry is synced before the handle is returned; callers
    /// must sync the file after completing writes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for an unsafe name or mode, an existing entry, or
    /// failure to create, set permissions, or sync the directory entry.
    pub fn create_new_file_writer(&self, name: &str, mode: u32) -> io::Result<File> {
        validate_basename(name)?;
        if mode & !0o777 != 0 {
            return Err(invalid_input(
                "file mode contains unsupported permission bits",
            ));
        }
        let raw_mode = RawMode::try_from(mode)
            .map_err(|_| invalid_input("file mode does not fit platform mode type"))?;
        let file = File::from(openat(
            &self.descriptor,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(raw_mode),
        )?);
        let result = (|| {
            fchmod(file.as_fd(), Mode::from_raw_mode(raw_mode))?;
            fsync(&self.descriptor)?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(file);
            let _ = unlinkat(&self.descriptor, name, AtFlags::empty());
            return Err(error);
        }
        Ok(file)
    }

    /// Store bytes under their lowercase SHA-256 digest, without replacing an
    /// existing object. A pre-existing entry is accepted only if it validates.
    ///
    /// Writes have no additional size cap, matching Rover's current Go store.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if writing, syncing, publication, or verification
    /// fails.
    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<Sha256Digest> {
        let digest = Sha256Digest::of(bytes).to_hex();
        let (temp_name, mut temp_file) = self.create_temp("object")?;
        let _cleanup = TempEntry::new(self, temp_name.clone());
        fchmod(temp_file.as_fd(), Mode::from(0o600))?;
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);

        match linkat(
            &self.descriptor,
            &temp_name,
            &self.descriptor,
            &digest,
            AtFlags::empty(),
        ) {
            Ok(()) => {}
            Err(error) if error == rustix::io::Errno::EXIST => {
                let existing = self.read_blob(&digest)?;
                if existing != bytes {
                    return Err(invalid_data(
                        "existing object bytes differ from digest input",
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }
        fsync(&self.descriptor)?;
        Sha256Digest::parse_hex(&digest)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
    }

    /// Read and verify a content-addressed object.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed digests, symlinks, non-regular files,
    /// objects over 16 MiB, and digest mismatches.
    pub fn read_blob(&self, digest: &str) -> io::Result<Vec<u8>> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_input("invalid object digest"));
        }
        let file = File::from(openat(
            &self.descriptor,
            digest,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = fstat(file.as_fd())?;
        if !FileType::from_raw_mode(metadata.st_mode).is_file() {
            return Err(invalid_data("object is not a regular file"));
        }
        if metadata.st_size < 0
            || u64::try_from(metadata.st_size).map_or(true, |size| size > MAX_BLOB_BYTES)
        {
            return Err(invalid_data("object too large"));
        }
        let capacity = usize::try_from(metadata.st_size)
            .map_err(|_| invalid_data("object size does not fit memory address space"))?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_BLOB_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(invalid_data("object too large"));
        }
        let actual = Sha256Digest::of(&bytes).to_hex();
        if actual != digest {
            return Err(invalid_data("object digest mismatch"));
        }
        Ok(bytes)
    }

    /// Write a bounded file without following a final-component symlink.
    /// Files are opened exclusively as temporary entries, assigned the exact
    /// requested permission bits, synced, atomically renamed, then directory
    /// synced. `name` must be a single normal path component.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the name or mode is invalid, or any filesystem
    /// operation fails.
    pub fn atomic_write(&self, name: &str, bytes: &[u8], mode: u32) -> io::Result<()> {
        validate_basename(name)?;
        if mode & !0o777 != 0 {
            return Err(invalid_input(
                "file mode contains unsupported permission bits",
            ));
        }
        let (temp_name, mut file) = self.create_temp("tmp")?;
        let _cleanup = TempEntry::new(self, temp_name.clone());
        let mode = RawMode::try_from(mode)
            .map_err(|_| invalid_input("file mode does not fit platform mode type"))?;
        fchmod(file.as_fd(), Mode::from_raw_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        renameat(&self.descriptor, &temp_name, &self.descriptor, name)?;
        fsync(&self.descriptor)?;
        Ok(())
    }

    /// Read at most `maximum` bytes from a no-follow regular file.
    ///
    /// Like Go's `BoundedFile`, this returns a truncated buffer when a regular
    /// file exceeds the requested bound.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for an invalid name, symlink, or non-regular file.
    /// A zero or negative bound returns an empty buffer after file validation,
    /// matching Go's `io.LimitReader` behavior.
    pub fn bounded_read(&self, name: &str, maximum: i64) -> io::Result<Vec<u8>> {
        validate_basename(name)?;
        let file = File::from(openat(
            &self.descriptor,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = fstat(file.as_fd())?;
        if !FileType::from_raw_mode(metadata.st_mode).is_file() {
            return Err(invalid_data("bounded input is not a regular file"));
        }
        let mut bytes = Vec::new();
        let limit = if maximum <= 0 {
            0
        } else {
            u64::try_from(maximum).map_err(|_| invalid_input("file bound overflow"))?
        };
        file.take(limit).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn create_temp(&self, prefix: &str) -> io::Result<(String, File)> {
        for _ in 0..128 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".{prefix}-{}-{sequence}", std::process::id());
            match openat(
                &self.descriptor,
                &name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from(0o600),
            ) {
                Ok(file) => {
                    return Ok((name, File::from(file)));
                }
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique temporary entry",
        ))
    }
}

struct TempEntry<'a> {
    directory: &'a SafeDir,
    name: String,
}

impl<'a> TempEntry<'a> {
    fn new(directory: &'a SafeDir, name: String) -> Self {
        Self { directory, name }
    }
}

impl Drop for TempEntry<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(&self.directory.descriptor, &self.name, AtFlags::empty());
    }
}

/// An admitted Rover state root with its `SQLite` records and private file areas.
///
/// Opening a state root initializes an empty directory or opens a validated
/// Rover database in place. Existing state is never deleted or migrated by
/// interpreting unknown data.
pub struct StateRoot {
    path: PathBuf,
    root: SafeDir,
    records: Store,
    objects: SafeDir,
    tasks: SafeDir,
    checks: SafeDir,
}

impl StateRoot {
    /// Admit or create a dedicated Unix Rover state directory and open its
    /// `SQLite` record store and private subdirectories.
    ///
    /// The root must not be a filesystem root, the current directory, or the
    /// home directory. A nonempty directory must contain a regular `rover.db`;
    /// its schema is then validated by the existing migration ledger.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unsafe paths, unrelated nonempty directories,
    /// unsafe file types/permissions, or SQLite/migration failures.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = canonical_state_path(path.as_ref())?;
        if path.parent().is_none() {
            return Err(invalid_input("state must be a dedicated directory"));
        }
        for base in [
            std::env::current_dir().ok(),
            std::env::var_os("HOME").map(PathBuf::from),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(base) = fs::canonicalize(base) {
                if base == path {
                    return Err(invalid_input(
                        "state must not be the home or current working directory",
                    ));
                }
            }
        }
        if std::env::temp_dir()
            .canonicalize()
            .is_ok_and(|temporary_root| temporary_root == path)
        {
            return Err(invalid_input(
                "state must be a dedicated directory below the temporary root",
            ));
        }

        match fs::read_dir(&path) {
            Ok(mut entries) => {
                if entries.next().transpose()?.is_some() {
                    let database = fs::symlink_metadata(path.join("rover.db"));
                    if !database.is_ok_and(|metadata| metadata.file_type().is_file()) {
                        return Err(invalid_data(
                            "nonempty state destination is not an existing Rover store",
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        fs::create_dir_all(&path)?;
        let root = open_private_directory(&path)?;
        let objects = open_private_directory(&path.join("objects"))?;
        let tasks = open_private_directory(&path.join("tasks"))?;
        let checks = open_private_directory(&path.join("checks"))?;
        prepare_database(&root)?;

        let connection = Connection::open_with_flags(
            path.join("rover.db"),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )
        .map_err(sqlite_error)?;
        let records = Store::from_connection(connection).map_err(store_error)?;

        Ok(Self {
            path,
            root,
            records,
            objects,
            tasks,
            checks,
        })
    }

    /// Return the canonical path of the admitted state root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Prepare and return the dedicated workspace path for a Rover task.
    /// The private task directory is created through the admitted `tasks`
    /// directory handle; the workspace leaf is intentionally left absent for
    /// `git worktree add` to create.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `owner` is not a valid Rover ID or the task
    /// directory cannot be opened/created safely.
    pub fn task_workspace_path(&self, owner: &str) -> io::Result<PathBuf> {
        rover_core::Id::parse(owner.to_owned())
            .map_err(|_| invalid_input("task owner is not a valid Rover ID"))?;
        self.tasks.ensure_subdirectory(owner)?;
        Ok(self.path.join("tasks").join(owner).join("workspace"))
    }

    /// Acquire a per-destination OS lock for Git worktree admission. The lock
    /// file lives in Rover's private task area and persists so concurrent
    /// processes always lock the same inode; closing the returned handle
    /// releases the lock, including during process exit.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `workspace` is not absolute or the private lock
    /// file cannot be opened and locked safely.
    pub fn lock_worktree_admission(&self, workspace: &Path) -> io::Result<File> {
        if !workspace.is_absolute() {
            return Err(invalid_input("worktree path must be absolute"));
        }
        let key = format!(
            "worktree-{}",
            Sha256Digest::of(workspace.as_os_str().as_encoded_bytes())
        );
        self.tasks.lock_exclusive_file(&key)
    }

    /// Borrow the transactional record and event store.
    #[must_use]
    pub const fn records(&self) -> &Store {
        &self.records
    }

    /// Store an artifact in the content-addressed objects directory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if writing, syncing, publication, or verification
    /// fails.
    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<Sha256Digest> {
        self.objects.put_blob(bytes)
    }

    /// Read and verify a content-addressed artifact.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the digest is invalid or the stored object
    /// fails type, size, or hash validation.
    pub fn read_blob(&self, digest: &str) -> io::Result<Vec<u8>> {
        self.objects.read_blob(digest)
    }

    /// Borrow the private task-output directory handle.
    #[must_use]
    pub const fn tasks(&self) -> &SafeDir {
        &self.tasks
    }

    /// Borrow the private check-output directory handle.
    #[must_use]
    pub const fn checks(&self) -> &SafeDir {
        &self.checks
    }

    /// Keep the state-root directory descriptor alive for the lifetime of this
    /// object, anchoring the stored database path's containing directory.
    #[must_use]
    pub const fn root_handle(&self) -> &SafeDir {
        &self.root
    }
}

/// Create or admit a private directory and return a descriptor anchored to it.
/// Existing ancestors are opened without following symlinks; only the final
/// directory is changed to mode `0700`. Filesystem roots, the shared temporary
/// root, the current directory, and the user's home directory are refused.
///
/// # Errors
///
/// Returns an I/O error if the path or one of its ancestors is unsafe, a
/// component is not a directory, or permissions cannot be applied.
pub fn prepare_private_directory(path: impl AsRef<Path>) -> io::Result<SafeDir> {
    let canonical = canonical_state_path(path.as_ref())?;
    if canonical.parent().is_none() {
        return Err(invalid_input(
            "private directory must not be a filesystem root",
        ));
    }
    let mut protected = vec![std::env::temp_dir()];
    if let Ok(current) = std::env::current_dir() {
        protected.push(current);
    }
    if let Some(home) = std::env::var_os("HOME") {
        protected.push(PathBuf::from(home));
    }
    for protected_path in protected {
        if canonical_state_path(&protected_path).is_ok_and(|path| path == canonical) {
            return Err(invalid_input(
                "private directory must not be the temporary root, current directory, or home",
            ));
        }
    }
    let mut directory = SafeDir::open_directory(Path::new("/"))?;
    for component in canonical.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or_else(|| invalid_input("directory path component is not UTF-8"))?;
                directory = directory.ensure_subdirectory(name)?;
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(invalid_input("directory path is not canonical"));
            }
        }
    }
    directory.set_permissions(0o700)?;
    Ok(directory)
}

pub(crate) fn canonical_state_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&std::env::current_dir()?.join(path))
    };
    let mut existing = absolute.as_path();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(metadata) => {
                if !metadata.file_type().is_symlink() && !metadata.is_dir() {
                    return Err(invalid_data("state path ancestor is not a directory"));
                }
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                existing = existing
                    .parent()
                    .ok_or_else(|| invalid_input("state path has no existing ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }

    let mut ancestor = Some(existing);
    while let Some(candidate) = ancestor {
        let metadata = fs::symlink_metadata(candidate)?;
        if metadata.file_type().is_symlink() {
            #[cfg(target_os = "macos")]
            if candidate == Path::new("/tmp") || candidate == Path::new("/var") {
                break;
            }
            return Err(invalid_data("state path has a symlinked ancestor"));
        }
        ancestor = candidate.parent();
    }

    let canonical_existing = fs::canonicalize(existing)?;
    let remaining = absolute
        .strip_prefix(existing)
        .map_err(|_| invalid_data("state path ancestor relationship changed"))?;
    Ok(normalize_path(&canonical_existing.join(remaining)))
}

fn open_private_directory(path: &Path) -> io::Result<SafeDir> {
    if !path.exists() {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let metadata = fstat(&descriptor)?;
    if !FileType::from_raw_mode(metadata.st_mode).is_dir() {
        return Err(invalid_data("state directory component is not a directory"));
    }
    fchmod(&descriptor, Mode::from(0o700))?;
    fsync(&descriptor)?;
    Ok(SafeDir { descriptor })
}

fn prepare_database(root: &SafeDir) -> io::Result<()> {
    match statat(&root.descriptor, "rover.db", AtFlags::SYMLINK_NOFOLLOW) {
        Ok(metadata) => {
            if !FileType::from_raw_mode(metadata.st_mode).is_file() {
                return Err(invalid_data("database must be a regular file"));
            }
        }
        Err(error) if error == rustix::io::Errno::NOENT => {
            match openat(
                &root.descriptor,
                "rover.db",
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from(0o600),
            ) {
                Ok(file) => {
                    File::from(file).sync_all()?;
                    fsync(&root.descriptor)?;
                }
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    let database = File::from(openat(
        &root.descriptor,
        "rover.db",
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let metadata = fstat(database.as_fd())?;
    if !FileType::from_raw_mode(metadata.st_mode).is_file() {
        return Err(invalid_data("database must be a regular file"));
    }
    fchmod(database.as_fd(), Mode::from(0o600))?;
    database.sync_all()
}

fn sqlite_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

fn store_error(error: StoreError) -> io::Error {
    io::Error::other(error)
}

/// Test whether `path` is lexically contained in `root`, using path components
/// rather than string prefixes. This does not resolve symlinks or touch disk.
#[must_use]
pub fn is_within(root: &Path, path: &Path) -> bool {
    let root = normalize_path(root);
    let path = normalize_path(path);
    path.starts_with(root)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn validate_basename(name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(invalid_input("file name contains a forbidden character"));
    }
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(invalid_input("file name must be one normal path component"));
    }
    Ok(())
}

fn relative_path_components(path: &str, allow_empty: bool) -> io::Result<Vec<&str>> {
    if path.is_empty() && allow_empty {
        return Ok(Vec::new());
    }
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(invalid_input("relative path is invalid"));
    }
    let components = path.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        component.is_empty()
            || matches!(*component, "." | "..")
            || component.eq_ignore_ascii_case(".git")
    }) {
        return Err(invalid_input("relative path contains an unsafe component"));
    }
    for component in &components {
        validate_basename(component)?;
    }
    Ok(components)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{is_within, prepare_private_directory, SafeDir, StateRoot};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rover-files-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create test directory");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("set private test directory");
            Self(path)
        }

        fn open(&self) -> SafeDir {
            SafeDir::open(&self.0).expect("open safe directory")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn prepare_private_directory_creates_and_hardens_only_the_leaf() {
        let directory = TestDirectory::new();
        let parent = directory.0.join("ordinary-parent");
        fs::create_dir(&parent).expect("create parent");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))
            .expect("make parent nonprivate");
        let leaf = parent.join("output");
        let anchored = prepare_private_directory(&leaf).expect("prepare output directory");
        assert_eq!(
            fs::metadata(&leaf)
                .expect("inspect leaf")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&parent)
                .expect("inspect parent")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(anchored.create_new_file("log", b"private", 0o600).is_ok());
        assert!(prepare_private_directory("/").is_err());
        assert!(prepare_private_directory(std::env::current_dir().unwrap()).is_err());
        assert!(prepare_private_directory(std::env::temp_dir()).is_err());
    }

    #[test]
    fn blobs_round_trip_and_validate_digest_names() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let digest = files
            .put_blob(b"content addressed")
            .expect("put blob")
            .to_hex();
        assert_eq!(digest.len(), 64);
        assert_eq!(
            fs::metadata(directory.0.join(&digest))
                .expect("blob metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            files.read_blob(&digest).expect("read blob"),
            b"content addressed"
        );
        assert_eq!(
            files
                .read_blob("../../outside")
                .expect_err("reject traversal")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            files
                .read_blob(&"A".repeat(64))
                .expect_err("reject uppercase digest")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn duplicate_concurrent_blob_publication_is_idempotent() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let payload = b"shared object".to_vec();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = directory.0.clone();
                let payload = payload.clone();
                std::thread::spawn(move || {
                    SafeDir::open(path)
                        .expect("open concurrent directory")
                        .put_blob(&payload)
                        .expect("concurrent blob write")
                })
            })
            .collect();
        let digests: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker completes"))
            .collect();
        assert!(digests.iter().all(|digest| digest == &digests[0]));
        assert_eq!(
            fs::read_dir(&directory.0)
                .expect("list blob directory")
                .count(),
            1,
            "temporary entries are removed"
        );
        assert_eq!(
            files
                .read_blob(&digests[0].to_hex())
                .expect("read published blob"),
            payload
        );
    }

    #[test]
    fn blob_symlinks_tampering_and_oversize_files_are_rejected() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let digest = files.put_blob(b"original").expect("put blob").to_hex();
        fs::write(directory.0.join(&digest), b"modified").expect("tamper blob");
        assert!(files.put_blob(b"original").is_err());
        assert_eq!(
            files
                .read_blob(&digest)
                .expect_err("reject tampering")
                .kind(),
            io::ErrorKind::InvalidData
        );

        fs::remove_file(directory.0.join(&digest)).expect("remove tampered file");
        let outside = directory.0.with_extension("outside");
        fs::write(&outside, b"outside").expect("write outside target");
        symlink(&outside, directory.0.join(&digest)).expect("plant blob symlink");
        assert!(files.read_blob(&digest).is_err());

        fs::remove_file(directory.0.join(&digest)).expect("remove blob symlink");
        fs::create_dir(directory.0.join(&digest)).expect("create object directory");
        assert_eq!(
            files
                .read_blob(&digest)
                .expect_err("reject object directory")
                .kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir(directory.0.join(&digest)).expect("remove object directory");
        fs::write(directory.0.join(&digest), vec![0; 16 * 1024 * 1024 + 1])
            .expect("write oversized object");
        assert_eq!(
            files
                .read_blob(&digest)
                .expect_err("reject oversized object")
                .kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn atomic_write_replaces_symlink_entry_with_requested_mode() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let outside = directory.0.with_extension("target");
        fs::write(&outside, b"leave alone").expect("write outside target");
        symlink(&outside, directory.0.join("output")).expect("plant target symlink");

        files
            .atomic_write("output", b"new content", 0o640)
            .expect("atomically replace target entry");
        assert_eq!(
            fs::read(&outside).expect("read external target"),
            b"leave alone"
        );
        assert_eq!(
            fs::read(directory.0.join("output")).expect("read output"),
            b"new content"
        );
        assert_eq!(
            fs::metadata(directory.0.join("output"))
                .expect("output metadata")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn blob_read_accepts_the_exact_sixteen_mibibyte_limit() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let payload = vec![0x5a; 16 * 1024 * 1024];
        let digest = files
            .put_blob(&payload)
            .expect("put blob at size limit")
            .to_hex();
        assert_eq!(files.read_blob(&digest).expect("read size limit"), payload);
    }

    #[test]
    fn safe_directory_rejects_links_and_public_permissions() {
        let directory = TestDirectory::new();
        let link = directory.0.with_extension("link");
        symlink(&directory.0, &link).expect("create directory symlink");
        assert!(SafeDir::open(&link).is_err());

        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755))
            .expect("make directory public");
        assert!(SafeDir::open(&directory.0).is_err());
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o500))
            .expect("remove owner write permission");
        assert!(SafeDir::open(&directory.0).is_err());
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))
            .expect("restore private mode");
        let _ = fs::remove_file(link);
    }

    #[test]
    fn directory_handles_create_nested_dirs_and_exclusive_files_without_following_links() {
        let directory = TestDirectory::new();
        let root = SafeDir::open_directory(&directory.0).expect("open output directory");
        let nested = root
            .ensure_subdirectory("nested")
            .expect("create nested output directory");
        nested
            .create_new_file("file.txt", b"frozen bytes", 0o755)
            .expect("create output file");
        assert_eq!(
            fs::read(directory.0.join("nested/file.txt")).unwrap(),
            b"frozen bytes"
        );
        assert_eq!(
            fs::metadata(directory.0.join("nested/file.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(nested
            .create_new_file("file.txt", b"replacement", 0o644)
            .is_err());

        let target = directory.0.join("outside.txt");
        fs::write(&target, b"outside").unwrap();
        symlink(&target, directory.0.join("nested/link.txt")).unwrap();
        assert!(nested
            .create_new_file("link.txt", b"must not follow", 0o600)
            .is_err());
        assert!(root.ensure_subdirectory("nested/link.txt").is_err());
        assert_eq!(fs::read(target).unwrap(), b"outside");
    }

    #[test]
    fn descriptor_relative_file_removal_rejects_symlinks_and_traversal() {
        let directory = TestDirectory::new();
        let root = directory.open();
        let nested = directory.0.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("remove.txt"), b"remove me").unwrap();
        let outside = directory.0.join("outside.txt");
        fs::write(&outside, b"keep me").unwrap();
        symlink(&outside, nested.join("link.txt")).unwrap();

        root.remove_regular_file_path("nested/remove.txt")
            .expect("remove regular nested file");
        assert!(!nested.join("remove.txt").exists());
        assert!(root.remove_regular_file_path("nested/link.txt").is_err());
        assert!(root.remove_regular_file_path("../outside.txt").is_err());
        assert_eq!(fs::read(outside).unwrap(), b"keep me");
    }

    #[test]
    fn output_directory_open_accepts_nonprivate_existing_root_without_following_final_link() {
        let directory = TestDirectory::new();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(SafeDir::open_directory(&directory.0).is_ok());
        assert!(SafeDir::open(&directory.0).is_err());
        let link = directory.0.with_extension("link");
        symlink(&directory.0, &link).unwrap();
        assert!(SafeDir::open_directory(&link).is_err());
        fs::remove_file(link).unwrap();
    }

    #[test]
    fn bounded_reads_truncate_and_reject_non_files_and_unsafe_names() {
        let directory = TestDirectory::new();
        let files = directory.open();
        fs::write(directory.0.join("input"), b"abcdef").expect("write input");
        assert_eq!(
            files.bounded_read("input", 4).expect("bounded read"),
            b"abcd"
        );
        assert_eq!(files.bounded_read("input", 0).expect("empty read"), b"");
        assert_eq!(
            files
                .bounded_read("input", -2)
                .expect("negative bound matches Go"),
            b""
        );
        assert!(files.bounded_read("../input", 3).is_err());
        assert!(files.bounded_read("./input", 3).is_err());
        symlink(directory.0.join("input"), directory.0.join("input-link"))
            .expect("create input symlink");
        assert!(files.bounded_read("input-link", 3).is_err());
        assert!(files.atomic_write("nested/path", b"x", 0o600).is_err());
        assert!(files.atomic_write("bad", b"x", 0o1600).is_err());
        assert!(files.bounded_read("missing", 2).is_err());
        fs::create_dir(directory.0.join("subdir")).expect("create directory");
        assert_eq!(
            files
                .bounded_read("subdir", 2)
                .expect_err("reject directory")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn repository_browser_lists_without_following_links_and_bounds_file_reads() {
        let directory = TestDirectory::new();
        let repo = directory.0.join("repo");
        fs::create_dir(&repo).expect("create repository root");
        fs::create_dir(repo.join("src")).expect("create source directory");
        fs::create_dir(repo.join(".git")).expect("create git metadata directory");
        fs::write(repo.join("src/main.rs"), b"hello world").expect("write source file");
        fs::write(repo.join(".secret"), b"hidden").expect("write hidden file");
        fs::write(repo.join(".git/config"), b"internal").expect("write git metadata");
        let outside = directory.0.join("outside");
        fs::create_dir(&outside).expect("create external directory");
        fs::write(outside.join("secret"), b"outside").expect("write external secret");
        symlink(&outside, repo.join("linked")).expect("create external directory symlink");
        let repository = SafeDir::open_directory(&repo).expect("open repository root");

        let (visible, truncated) = repository
            .list_directory_path("", &repo, false, 20)
            .expect("list visible root entries");
        assert!(!truncated);
        assert_eq!(
            visible
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["linked", "src"]
        );
        assert_eq!(visible[0].kind, super::SafeEntryKind::Symlink);
        assert_eq!(visible[1].kind, super::SafeEntryKind::Directory);
        let (mismatched_path, _) = repository
            .list_directory_path("", &outside, true, 20)
            .expect("validate enumerated names against opened directory");
        assert!(mismatched_path.is_empty());

        let (with_hidden, _) = repository
            .list_directory_path("", &repo, true, 20)
            .expect("list hidden root entries");
        assert!(with_hidden.iter().any(|entry| entry.name == ".secret"));
        assert!(!with_hidden.iter().any(|entry| entry.name == ".git"));

        let (bytes, truncated) = repository
            .read_regular_file_path("src/main.rs", 5)
            .expect("read bounded source preview");
        assert_eq!(bytes, b"hello");
        assert!(truncated);
        assert!(repository
            .read_regular_file_path("linked/secret", 32)
            .is_err());
        assert!(repository
            .read_regular_file_path("../outside/secret", 32)
            .is_err());
        assert!(repository
            .list_directory_path("linked", &repo.join("linked"), true, 20)
            .is_err());
        assert!(repository.list_directory_path("", &repo, true, 0).is_err());
    }

    #[test]
    fn lexical_containment_uses_components_and_normalizes_parent_segments() {
        assert!(is_within(
            Path::new("/tmp/state"),
            Path::new("/tmp/state/object")
        ));
        assert!(is_within(
            Path::new("/tmp/state"),
            Path::new("/tmp/state/nested/../object")
        ));
        assert!(!is_within(
            Path::new("/tmp/state"),
            Path::new("/tmp/state-old/file")
        ));
        assert!(!is_within(
            Path::new("/tmp/state"),
            Path::new("/tmp/elsewhere")
        ));
    }

    #[test]
    fn opened_directory_handle_survives_path_renaming() {
        let directory = TestDirectory::new();
        let files = directory.open();
        let moved = directory.0.with_extension("moved");
        fs::rename(&directory.0, &moved).expect("rename opened directory");
        files
            .atomic_write("entry", b"anchored", 0o600)
            .expect("write through open directory handle");
        assert_eq!(
            fs::read(moved.join("entry")).expect("read anchored file"),
            b"anchored"
        );
        fs::rename(&moved, &directory.0).expect("restore test directory");
    }

    #[test]
    fn state_root_opens_records_and_private_file_areas_across_reopen() {
        let directory = TestDirectory::new();
        let state_path = directory.0.join("rover-state");
        let canonical = {
            let root = StateRoot::open(&state_path).expect("open new state root");
            assert_eq!(
                root.path(),
                fs::canonicalize(&state_path).expect("canonical state path")
            );
            root.records()
                .put("task", "task_123", &serde_json::json!({"value": 7}), "")
                .expect("write state record");
            let digest = root.put_blob(b"state-root blob").expect("write blob");
            assert_eq!(
                root.read_blob(&digest.to_hex()).expect("read blob"),
                b"state-root blob"
            );
            root.tasks()
                .atomic_write("stdout", b"task output", 0o600)
                .expect("write task output");
            root.checks()
                .atomic_write("report", b"check report", 0o600)
                .expect("write check output");

            for name in ["objects", "tasks", "checks"] {
                assert_eq!(
                    fs::metadata(state_path.join(name))
                        .expect("directory metadata")
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
            assert_eq!(
                fs::metadata(state_path.join("rover.db"))
                    .expect("database metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            root.path().to_owned()
        };

        let reopened = StateRoot::open(&state_path).expect("reopen state root");
        assert_eq!(reopened.path(), canonical);
        let record = reopened
            .records()
            .get("task", "task_123")
            .expect("read record after reopen");
        assert_eq!(record["value"], 7);
        assert_eq!(
            reopened
                .tasks()
                .bounded_read("stdout", 100)
                .expect("read task output"),
            b"task output"
        );
    }

    #[test]
    fn task_workspace_path_prepares_private_parent_but_leaves_git_leaf_absent() {
        let directory = TestDirectory::new();
        let state_path = directory.0.join("state");
        let state = StateRoot::open(&state_path).expect("open state root");
        let workspace = state
            .task_workspace_path("task_0123456789abcdef")
            .expect("prepare task worktree parent");
        assert_eq!(
            workspace,
            state.path().join("tasks/task_0123456789abcdef/workspace")
        );
        assert!(!workspace.exists(), "Git worktree leaf must remain absent");
        assert_eq!(
            fs::metadata(workspace.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(state.task_workspace_path("../outside").is_err());
    }

    #[test]
    fn worktree_admission_lock_is_released_when_owner_panics() {
        let directory = TestDirectory::new();
        let state_path = directory.0.join("state");
        let state = Arc::new(StateRoot::open(&state_path).expect("open state root"));
        let workspace = state
            .task_workspace_path("task-lock-test")
            .expect("prepare task workspace");
        let (locked_sender, locked_receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker_workspace = workspace.clone();
        let worker = thread::spawn(move || {
            let _lock = worker_state
                .lock_worktree_admission(&worker_workspace)
                .expect("acquire admission lock");
            locked_sender.send(()).expect("signal lock acquisition");
            panic!("simulate owner crash while holding OS lock");
        });
        locked_receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("worker acquired OS lock");
        assert!(worker.join().is_err(), "fixture worker should panic");

        let reacquired = state
            .lock_worktree_admission(&workspace)
            .expect("OS releases lock when owner unwinds");
        drop(reacquired);
    }

    #[test]
    fn state_root_refuses_unrelated_nonempty_directories_without_mutating_them() {
        let directory = TestDirectory::new();
        let unrelated = directory.0.join("unrelated");
        fs::create_dir(&unrelated).expect("create unrelated directory");
        fs::set_permissions(&unrelated, fs::Permissions::from_mode(0o755))
            .expect("make unrelated directory public");
        fs::write(unrelated.join("keep.txt"), b"keep").expect("write unrelated file");

        assert!(StateRoot::open(&unrelated).is_err());
        assert_eq!(
            fs::metadata(&unrelated)
                .expect("unrelated metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(!unrelated.join("objects").exists());
    }

    #[test]
    fn state_root_refuses_the_shared_temporary_root_without_changing_it() {
        let temporary_root = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temporary root");
        let old_mode = fs::metadata(&temporary_root)
            .expect("temporary root metadata")
            .permissions()
            .mode()
            & 0o777;
        assert!(StateRoot::open(&temporary_root).is_err());
        assert_eq!(
            fs::metadata(&temporary_root)
                .expect("temporary root metadata after refusal")
                .permissions()
                .mode()
                & 0o777,
            old_mode
        );
    }

    #[test]
    fn state_root_rejects_symlink_components_and_database_symlinks() {
        let directory = TestDirectory::new();
        let target = directory.0.join("target");
        fs::create_dir(&target).expect("create target directory");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
            .expect("make target private");
        let link = directory.0.join("linked-root");
        symlink(&target, &link).expect("create root symlink");
        assert!(StateRoot::open(&link).is_err());
        assert!(StateRoot::open(link.join("nested")).is_err());

        let database_root = directory.0.join("database-link");
        fs::create_dir(&database_root).expect("create database-link root");
        fs::set_permissions(&database_root, fs::Permissions::from_mode(0o700))
            .expect("make database-link root private");
        fs::write(directory.0.join("outside.db"), b"not a database").expect("write outside db");
        symlink(
            directory.0.join("outside.db"),
            database_root.join("rover.db"),
        )
        .expect("plant database symlink");
        assert!(StateRoot::open(&database_root).is_err());
    }

    #[test]
    fn state_root_refuses_a_future_database_schema() {
        let directory = TestDirectory::new();
        let state_path = directory.0.join("future-state");
        drop(StateRoot::open(&state_path).expect("create database"));
        let connection =
            rusqlite::Connection::open(state_path.join("rover.db")).expect("open test database");
        connection
            .pragma_update(None, "user_version", 99_i64)
            .expect("set future schema");
        drop(connection);
        assert!(StateRoot::open(&state_path).is_err());
    }
}
