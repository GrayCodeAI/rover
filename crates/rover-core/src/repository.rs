//! Canonical local repository identity and state/project audience binding.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::Sha256Digest;

/// A repository path and the stable project digest derived from it.
///
/// Resolution matches Rover's Go access layer: an absolute cleaned path is
/// used when full symlink resolution fails, so not-yet-created repositories
/// still have an identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryIdentity {
    canonical_path: PathBuf,
    project_id: Sha256Digest,
}

impl RepositoryIdentity {
    /// Resolve a repository path and compute its project ID.
    ///
    /// Empty input retains an empty canonical path. Nonempty input is made
    /// absolute and lexically clean; if the complete path exists and resolves,
    /// the filesystem's canonical path is used.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the current directory cannot be obtained for a
    /// relative input.
    pub fn resolve(repository: &str) -> io::Result<Self> {
        let canonical_path = if repository.is_empty() {
            PathBuf::new()
        } else {
            let repository = Path::new(repository);
            let absolute = if repository.is_absolute() {
                Some(repository.to_path_buf())
            } else {
                match std::env::current_dir() {
                    Ok(current) => Some(current.join(repository)),
                    // Go's filepath.Abs returns an error here and the Rover
                    // caller retains the original repository string.
                    Err(_) => None,
                }
            };
            if let Some(absolute) = absolute {
                let cleaned = normalize_path(&absolute);
                fs::canonicalize(&cleaned).unwrap_or(cleaned)
            } else {
                repository.to_path_buf()
            }
        };
        let project_id = Sha256Digest::of(canonical_path.as_os_str().as_encoded_bytes());
        Ok(Self {
            canonical_path,
            project_id,
        })
    }

    /// Return the resolved repository path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    /// Return this project's SHA-256 identity.
    #[must_use]
    pub const fn project_id(&self) -> Sha256Digest {
        self.project_id
    }
}

/// Derive the control audience bound to both state root and repository.
///
/// This preserves the Go wire format: `rover-control/v1:` followed by the
/// lowercase SHA-256 of `state_root`, one NUL byte, and the repository path.
#[must_use]
pub fn control_audience(state_root: &Path, repository: &RepositoryIdentity) -> String {
    let state_root = state_root.as_os_str().as_encoded_bytes();
    let repository = repository.canonical_path.as_os_str().as_encoded_bytes();
    let mut framed = Vec::with_capacity(state_root.len() + repository.len() + 1);
    framed.extend_from_slice(state_root);
    framed.push(0);
    framed.extend_from_slice(repository);
    format!("rover-control/v1:{}", Sha256Digest::of(&framed))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{control_audience, normalize_path, RepositoryIdentity};
    use crate::Sha256Digest;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rover-project-id-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn empty_and_unresolved_paths_follow_go_identity_fallback() {
        let empty = RepositoryIdentity::resolve("").expect("resolve empty path");
        assert_eq!(empty.path(), PathBuf::new());
        assert_eq!(empty.project_id(), Sha256Digest::of(b""));

        let directory = TestDirectory::new();
        let unresolved = directory.0.join("new-repository/../future");
        let identity =
            RepositoryIdentity::resolve(unresolved.to_str().expect("UTF-8 fixture path"))
                .expect("resolve missing path");
        assert_eq!(identity.path(), normalize_path(&unresolved));
        assert_eq!(
            identity.project_id(),
            Sha256Digest::of(identity.path().as_os_str().as_encoded_bytes())
        );
    }

    #[test]
    #[cfg(unix)]
    fn existing_path_variants_and_symlink_aliases_share_project_identity() {
        let directory = TestDirectory::new();
        let repository = directory.0.join("repo");
        fs::create_dir(&repository).expect("create repository");
        let alias = directory.0.join("repo-alias");
        symlink(&repository, &alias).expect("create repository symlink");

        let path = repository.to_str().expect("UTF-8 repository");
        let trailing = format!("{path}/./");
        let direct = RepositoryIdentity::resolve(path).expect("resolve direct path");
        let variant = RepositoryIdentity::resolve(&trailing).expect("resolve path variant");
        let linked = RepositoryIdentity::resolve(alias.to_str().expect("UTF-8 alias"))
            .expect("resolve symlink alias");
        assert_eq!(
            direct.path(),
            fs::canonicalize(&repository).expect("canonical repository")
        );
        assert_eq!(direct.project_id(), variant.project_id());
        assert_eq!(direct.project_id(), linked.project_id());
    }

    #[test]
    #[cfg(unix)]
    fn unresolved_symlink_descendants_use_clean_absolute_fallback() {
        let directory = TestDirectory::new();
        let target = directory.0.join("target");
        fs::create_dir(&target).expect("create target");
        let alias = directory.0.join("alias");
        symlink(&target, &alias).expect("create alias");
        let unresolved_alias_child = alias.join("future");
        let unresolved_target_child = target.join("future");
        let alias_identity = RepositoryIdentity::resolve(
            unresolved_alias_child.to_str().expect("UTF-8 alias child"),
        )
        .expect("resolve missing alias child");
        let target_identity = RepositoryIdentity::resolve(
            unresolved_target_child
                .to_str()
                .expect("UTF-8 target child"),
        )
        .expect("resolve missing target child");
        assert_eq!(
            alias_identity.path(),
            normalize_path(&unresolved_alias_child)
        );
        assert_ne!(alias_identity.project_id(), target_identity.project_id());
    }

    #[test]
    fn control_audience_binds_state_root_and_repository() {
        let directory = TestDirectory::new();
        let repository_path = directory.0.join("repo");
        fs::create_dir(&repository_path).expect("create repository");
        let repository =
            RepositoryIdentity::resolve(repository_path.to_str().expect("UTF-8 repository"))
                .expect("resolve repository");
        let first = control_audience(PathBuf::from("/state/one").as_path(), &repository);
        let same = control_audience(PathBuf::from("/state/one").as_path(), &repository);
        let other_state = control_audience(PathBuf::from("/state/two").as_path(), &repository);
        let other_repository =
            RepositoryIdentity::resolve("/another/project").expect("resolve other project");
        let other_project =
            control_audience(PathBuf::from("/state/one").as_path(), &other_repository);
        assert_eq!(first, same);
        assert_ne!(first, other_state);
        assert_ne!(first, other_project);
        assert!(first.starts_with("rover-control/v1:"));
    }
}
