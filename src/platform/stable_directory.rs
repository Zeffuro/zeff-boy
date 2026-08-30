use std::fs::{self, File, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

/// Detects directory replacement within a trusted same-user parent boundary.
/// It does not prevent hostile races on names in a writable parent.
#[derive(Clone, Debug)]
pub(crate) struct StableDirectory {
    path: PathBuf,
    handle: Arc<File>,
    identity: DirectoryIdentity,
    label: &'static str,
}

impl StableDirectory {
    pub(crate) fn open_or_create(path: &Path, label: &'static str) -> Result<Self> {
        prepare_directory(path, label)?;
        let canonical = fs::canonicalize(path)
            .with_context(|| format!("failed to resolve {label} {}", path.display()))?;
        let requested_metadata = checked_metadata(path, label)?;
        let canonical_metadata = checked_metadata(&canonical, label)?;
        let handle = open_directory(&canonical)?;
        let identity = directory_identity(&handle)?;
        let requested_handle = open_directory(path)?;
        if directory_identity(&requested_handle)? != identity
            || !canonical_metadata.is_dir()
            || !requested_metadata.is_dir()
            || fs::canonicalize(path)? != canonical
        {
            bail!(
                "{label} changed while it was being opened: {}",
                path.display()
            );
        }

        let directory = Self {
            path: canonical,
            handle: Arc::new(handle),
            identity,
            label,
        };
        directory.revalidate()?;
        Ok(directory)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn revalidate(&self) -> Result<()> {
        let metadata = checked_metadata(&self.path, self.label)?;
        let current_handle = open_directory(&self.path)?;
        if directory_identity(&current_handle)? != self.identity
            || directory_identity(&self.handle)? != self.identity
            || !metadata.is_dir()
        {
            bail!(
                "{} was replaced after it was opened: {}",
                self.label,
                self.path.display()
            );
        }
        let canonical = fs::canonicalize(&self.path).with_context(|| {
            format!(
                "failed to reauthenticate {} {}",
                self.label,
                self.path.display()
            )
        })?;
        if canonical != self.path {
            bail!(
                "{} no longer resolves to its opened canonical path: {}",
                self.label,
                self.path.display()
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    filesystem: u64,
    object: u64,
}

fn prepare_directory(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        bail!("{label} must be a dedicated named directory");
    }

    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create trusted parent for {label} {}",
                    parent.display()
                )
            })?;
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to create dedicated {label} {}", path.display())
                    });
                }
            }
        }
        Err(error) => return Err(error.into()),
    }

    checked_metadata(path, label).map(|_| ())
}

fn checked_metadata(path: &Path, label: &str) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {label} {}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata_is_redirect(&metadata) {
        bail!(
            "{label} must not be a symbolic link or reparse point: {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        bail!("{label} is not a directory: {}", path.display());
    }
    Ok(metadata)
}

#[cfg(not(windows))]
fn open_directory(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("failed to open stable directory {}", path.display()))
}

#[cfg(windows)]
fn open_directory(path: &Path) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("failed to open stable directory {}", path.display()))
}

#[cfg(unix)]
fn directory_identity(directory: &File) -> Result<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.metadata()?;
    Ok(DirectoryIdentity {
        filesystem: metadata.dev(),
        object: metadata.ino(),
    })
}

#[cfg(windows)]
fn directory_identity(directory: &File) -> Result<DirectoryIdentity> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is live and `information` is writable for the call.
    if unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &raw mut information) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to read stable directory identity");
    }
    Ok(DirectoryIdentity {
        filesystem: u64::from(information.dwVolumeSerialNumber),
        object: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &File) -> Result<DirectoryIdentity> {
    bail!("stable directory identity is unsupported on this platform")
}

#[cfg(windows)]
pub(crate) fn metadata_is_redirect(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn metadata_is_redirect(_metadata: &Metadata) -> bool {
    false
}
