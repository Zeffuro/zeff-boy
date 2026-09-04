use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;

const PCE_STATE_EXTENSION: &str = "pcestate";
const COLECO_STATE_EXTENSION: &str = "colstate";
const TEMP_FILE_ATTEMPTS: u32 = 64;

pub(super) fn write_pce_state_artifact(
    requested_path: &Path,
    bytes: &[u8],
) -> anyhow::Result<PathBuf> {
    let path = resolve_pce_state_artifact_path(requested_path)?;
    write_new_state_file(&path, bytes)?;
    Ok(path)
}

pub(super) fn write_coleco_state_artifact(
    requested_path: &Path,
    bytes: &[u8],
) -> anyhow::Result<PathBuf> {
    let path = resolve_state_artifact_path(requested_path, COLECO_STATE_EXTENSION, "ColecoVision")?;
    write_new_state_file(&path, bytes)?;
    Ok(path)
}

fn resolve_pce_state_artifact_path(path: &Path) -> anyhow::Result<PathBuf> {
    resolve_state_artifact_path(path, PCE_STATE_EXTENSION, "PC Engine")
}

fn resolve_state_artifact_path(
    path: &Path,
    expected_extension: &str,
    system: &str,
) -> anyhow::Result<PathBuf> {
    let current_dir = std::env::current_dir().context("failed to resolve current directory")?;
    let repo_root = workspace_root_from(&current_dir)?;
    resolve_state_artifact_path_from_root(&repo_root, path, expected_extension, system)
}

#[cfg(test)]
fn resolve_pce_state_artifact_path_from_root(
    workspace_root: &Path,
    path: &Path,
) -> anyhow::Result<PathBuf> {
    resolve_state_artifact_path_from_root(workspace_root, path, PCE_STATE_EXTENSION, "PC Engine")
}

fn resolve_state_artifact_path_from_root(
    workspace_root: &Path,
    path: &Path,
    expected_extension: &str,
    system: &str,
) -> anyhow::Result<PathBuf> {
    let repo_root = fs::canonicalize(workspace_root).with_context(|| {
        format!(
            "failed to canonicalize workspace root: {}",
            workspace_root.display()
        )
    })?;
    anyhow::ensure!(
        is_workspace_root(&repo_root),
        "{system} state artifacts require a Git workspace root with Cargo.toml"
    );
    let lexical_absolute = lexical_normalize_path(if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    });
    let absolute = if path.is_absolute() {
        let parent = lexical_absolute
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .context("state artifact path must have a parent directory")?;
        let file_name = lexical_absolute
            .file_name()
            .context("state artifact path must have a file name")?;
        fs::canonicalize(parent)
            .with_context(|| {
                format!(
                    "{system} state artifact parent must already exist: {}",
                    parent.display()
                )
            })?
            .join(file_name)
    } else {
        lexical_absolute
    };
    let allowed_roots = [
        repo_root.join("target"),
        repo_root.join("temp"),
        repo_root.join("rom-tests").join("results"),
    ];
    let Some(allowed_root) = allowed_roots.iter().find(|root| absolute.starts_with(root)) else {
        anyhow::bail!(
            "{system} state artifacts must be under ignored target/, temp/, or rom-tests/results/"
        );
    };
    if absolute
        .extension()
        .and_then(|extension| extension.to_str())
        != Some(expected_extension)
    {
        anyhow::bail!("{system} state artifacts must use the .{expected_extension} extension");
    }

    let canonical_allowed_root = fs::canonicalize(allowed_root).with_context(|| {
        format!(
            "{system} state artifact root must already exist: {}",
            allowed_root.display()
        )
    })?;
    anyhow::ensure!(
        canonical_allowed_root.starts_with(&repo_root),
        "{system} state artifact root escapes the workspace: {}",
        allowed_root.display()
    );
    let parent = absolute
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("state artifact path must have a parent directory")?;
    let canonical_parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "{system} state artifact parent must already exist: {}",
            parent.display()
        )
    })?;
    anyhow::ensure!(
        canonical_parent.starts_with(&canonical_allowed_root),
        "{system} state artifact parent escapes its allowed root: {}",
        parent.display()
    );

    Ok(absolute)
}

fn workspace_root_from(start: &Path) -> anyhow::Result<PathBuf> {
    let canonical_start = fs::canonicalize(start).with_context(|| {
        format!(
            "failed to canonicalize current directory: {}",
            start.display()
        )
    })?;
    canonical_start
        .ancestors()
        .find(|candidate| is_workspace_root(candidate))
        .map(Path::to_path_buf)
        .context("state artifacts require running from a Git workspace")
}

fn is_workspace_root(path: &Path) -> bool {
    path.join("Cargo.toml").is_file() && path.join(".git").exists()
}

fn lexical_normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(super) fn write_new_state_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("state artifact path must have a parent directory")?;
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .context("state artifact path must have a UTF-8 file name")?;
    if path
        .try_exists()
        .with_context(|| format!("failed to inspect state artifact: {}", path.display()))?
    {
        anyhow::bail!(
            "refusing to overwrite existing state artifact: {}",
            path.display()
        );
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..TEMP_FILE_ATTEMPTS {
        let temp_path = parent.join(format!(
            ".{file_name}.tmp.{}.{}.{attempt}",
            std::process::id(),
            nonce,
        ));
        let file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to create state artifact temp file: {}",
                        temp_path.display()
                    )
                });
            }
        };

        let write_result = (|| -> anyhow::Result<()> {
            let mut file = file;
            file.write_all(bytes).with_context(|| {
                format!(
                    "failed to write state artifact temp file: {}",
                    temp_path.display()
                )
            })?;
            file.sync_all().with_context(|| {
                format!(
                    "failed to flush state artifact temp file: {}",
                    temp_path.display()
                )
            })?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        if let Err(error) = fs::hard_link(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error).with_context(|| {
                format!(
                    "failed to finalize state artifact without overwrite: {}",
                    path.display()
                )
            });
        }
        if let Err(error) = fs::remove_file(&temp_path) {
            return Err(error).with_context(|| {
                format!(
                    "state artifact finalized at {} but could not remove owned temp link: {}",
                    path.display(),
                    temp_path.display()
                )
            });
        }
        return Ok(());
    }

    anyhow::bail!(
        "failed to reserve a unique state artifact temp file beside: {}",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDirectory, test_directory};

    fn test_workspace(label: &str) -> TestDirectory {
        let workspace = test_directory(label).unwrap();
        fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::create_dir(workspace.path().join(".git")).unwrap();
        fs::create_dir(workspace.path().join("target")).unwrap();
        fs::create_dir(workspace.path().join("temp")).unwrap();
        fs::create_dir_all(workspace.path().join("rom-tests").join("results")).unwrap();
        workspace
    }

    #[test]
    fn artifact_path_policy_anchors_subdirectories_to_workspace_root() {
        let workspace = test_workspace("workspace-root");
        let subdirectory = workspace.path().join("src").join("cli");
        fs::create_dir_all(&subdirectory).unwrap();

        let root = workspace_root_from(&subdirectory).unwrap();
        assert_eq!(root, fs::canonicalize(workspace.path()).unwrap());
        let resolved = resolve_pce_state_artifact_path_from_root(
            &root,
            Path::new("target/checkpoint.pcestate"),
        )
        .unwrap();
        assert_eq!(resolved, root.join("target").join("checkpoint.pcestate"));
    }

    #[test]
    fn artifact_path_policy_rejects_non_workspace_current_directory() {
        let outside = test_directory("outside-workspace").unwrap();
        assert!(workspace_root_from(outside.path()).is_err());
    }

    #[test]
    fn artifact_path_policy_normalizes_absolute_paths_before_workspace_check() {
        let workspace = test_workspace("absolute-path");
        let inside = workspace.path().join("target").join("checkpoint.pcestate");
        let resolved =
            resolve_pce_state_artifact_path_from_root(workspace.path(), &inside).unwrap();
        assert_eq!(
            resolved,
            fs::canonicalize(workspace.path().join("target"))
                .unwrap()
                .join("checkpoint.pcestate")
        );

        let outside = test_directory("absolute-outside").unwrap();
        assert!(
            resolve_pce_state_artifact_path_from_root(
                workspace.path(),
                &outside.path().join("checkpoint.pcestate"),
            )
            .is_err()
        );
    }

    #[test]
    fn artifact_path_policy_allows_ignored_roots_and_rejects_tracked_escaped_or_missing_paths() {
        let workspace = test_workspace("policy");
        for path in [
            "target/checkpoint.pcestate",
            "temp/checkpoint.pcestate",
            "rom-tests/results/checkpoint.pcestate",
        ] {
            assert!(
                resolve_pce_state_artifact_path_from_root(workspace.path(), Path::new(path))
                    .is_ok(),
                "{path}"
            );
        }
        for path in [
            "src/checkpoint.pcestate",
            "target/../src/checkpoint.pcestate",
            "target/checkpoint.state",
            "target/new-parent/checkpoint.pcestate",
        ] {
            assert!(
                resolve_pce_state_artifact_path_from_root(workspace.path(), Path::new(path))
                    .is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn coleco_artifacts_keep_the_same_safe_path_policy_and_colstate_extension() {
        let workspace = test_workspace("coleco-policy");
        assert!(
            resolve_state_artifact_path_from_root(
                workspace.path(),
                Path::new("target/checkpoint.colstate"),
                COLECO_STATE_EXTENSION,
                "ColecoVision",
            )
            .is_ok()
        );
        assert!(
            resolve_state_artifact_path_from_root(
                workspace.path(),
                Path::new("target/checkpoint.pcestate"),
                COLECO_STATE_EXTENSION,
                "ColecoVision",
            )
            .is_err()
        );
    }

    #[cfg(any(unix, windows))]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link)
        }
    }

    #[cfg(any(unix, windows))]
    fn create_directory_link_or_skip(target: &Path, link: &Path) -> bool {
        match create_directory_link(target, link) {
            Ok(()) => true,
            Err(error) if cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied => {
                false
            }
            Err(error) => panic!("failed to create directory link: {error}"),
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn artifact_path_policy_rejects_symlinked_allowed_roots_and_parents() {
        let outside = test_directory("symlink-outside").unwrap();

        let root_link_workspace = test_workspace("symlink-root");
        let root_link = root_link_workspace.path().join("target");
        fs::remove_dir(&root_link).unwrap();
        if !create_directory_link_or_skip(outside.path(), &root_link) {
            return;
        }
        assert!(
            resolve_pce_state_artifact_path_from_root(
                root_link_workspace.path(),
                Path::new("target/checkpoint.pcestate"),
            )
            .is_err()
        );

        let parent_link_workspace = test_workspace("symlink-parent");
        let parent_link = parent_link_workspace.path().join("target").join("escape");
        if !create_directory_link_or_skip(outside.path(), &parent_link) {
            return;
        }
        assert!(
            resolve_pce_state_artifact_path_from_root(
                parent_link_workspace.path(),
                Path::new("target/escape/checkpoint.pcestate"),
            )
            .is_err()
        );
    }

    #[test]
    fn write_new_state_file_roundtrips_bytes_and_preserves_existing_destination() {
        let temp = test_directory("state-artifact-write").unwrap();
        let path = temp.path().join("endpoint.pcestate");
        let bytes = [0x5A, 0x45, 0x46, 0x46];
        write_new_state_file(&path, &bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);

        fs::write(&path, b"keep-me").unwrap();
        let error = write_new_state_file(&path, b"replacement").unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(fs::read(&path).unwrap(), b"keep-me");
    }
}
