use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomicWritePhase {
    BeforeTemp,
    AfterTemp,
    AfterPartialWrite,
    AfterWrite,
    AfterSync,
    BeforeReplace,
    AfterReplace,
}

pub(crate) fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    write_file_atomically_with(path, bytes, |_| Ok(()), replace_file_by_path, |_| Ok(()))
}

pub(crate) fn write_file_atomically_validated(
    path: &Path,
    bytes: &[u8],
    validate: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    write_file_atomically_with(path, bytes, validate, replace_validated_file, |_| Ok(()))
}

pub(crate) fn write_new_file_atomically_validated(
    path: &Path,
    bytes: &[u8],
    validate: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    write_file_atomically_with(path, bytes, validate, publish_new_file, |_| Ok(()))
}

fn write_file_atomically_with(
    path: &Path,
    bytes: &[u8],
    validate: impl FnOnce(&mut File) -> Result<()>,
    replace: impl FnOnce(&File, &Path, &Path) -> std::io::Result<()>,
    mut checkpoint: impl FnMut(AtomicWritePhase) -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    checkpoint(AtomicWritePhase::BeforeTemp)?;

    let (temp_path, file) = create_sibling_temp(path)?;
    let mut file = Some(file);
    let result = (|| -> Result<()> {
        checkpoint(AtomicWritePhase::AfterTemp)?;
        let midpoint = bytes.len() / 2;
        file.as_mut()
            .expect("temporary file should remain open")
            .write_all(&bytes[..midpoint])
            .with_context(|| format!("failed to write temp file: {}", temp_path.display()))?;
        checkpoint(AtomicWritePhase::AfterPartialWrite)?;
        file.as_mut()
            .expect("temporary file should remain open")
            .write_all(&bytes[midpoint..])
            .with_context(|| format!("failed to write temp file: {}", temp_path.display()))?;
        checkpoint(AtomicWritePhase::AfterWrite)?;
        file.as_ref()
            .expect("temporary file should remain open")
            .sync_all()
            .with_context(|| format!("failed to flush temp file: {}", temp_path.display()))?;
        checkpoint(AtomicWritePhase::AfterSync)?;
        validate(file.as_mut().expect("temporary file should remain open"))?;
        checkpoint(AtomicWritePhase::BeforeReplace)?;
        validate_file_bytes(
            file.as_mut().expect("temporary file should remain open"),
            bytes,
        )?;
        replace(
            file.as_ref().expect("temporary file should remain open"),
            &temp_path,
            path,
        )
        .with_context(|| format!("failed to replace file: {}", path.display()))?;
        drop(file.take());
        checkpoint(AtomicWritePhase::AfterReplace)?;
        sync_parent(path)
            .with_context(|| format!("failed to sync directory for file: {}", path.display()))?;
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn create_sibling_temp(target: &Path) -> Result<(PathBuf, File)> {
    let file_name = target
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("target has no file name"))?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(file_name);
        temp_name.push(format!(".tmp.{}.{sequence}", std::process::id()));
        let temp_path = parent.join(temp_name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
            use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_SHARE_READ};

            options.access_mode(GENERIC_READ | GENERIC_WRITE | DELETE);
            options.share_mode(FILE_SHARE_READ);
        }
        match options.open(&temp_path) {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("could not reserve a temporary file")
}

fn validate_file_bytes(file: &mut File, expected: &[u8]) -> Result<()> {
    file.rewind()?;
    let mut actual = Vec::new();
    file.take(
        u64::try_from(expected.len())
            .map_err(|_| anyhow::anyhow!("atomic file length does not fit u64"))?
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("atomic file length overflow"))?,
    )
    .read_to_end(&mut actual)?;
    if actual != expected {
        bail!("validated temporary file changed before publication");
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn replace_validated_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    let guard = create_descriptor_link(source_file, target)?;
    let result = std::fs::rename(&guard, target);
    if result.is_err() {
        let _ = std::fs::remove_file(&guard);
    }
    result?;
    remove_source_path(source)
}

#[cfg(target_os = "macos")]
fn replace_validated_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    let (guard, guard_file) = create_macos_publication_guard(source_file, target)?;
    let result = macos_rename_guard(&guard, target, 0);
    drop(guard_file);
    if result.is_err() {
        let _ = std::fs::remove_file(&guard);
    }
    result?;
    remove_source_path(source)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn publish_new_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    link_descriptor_to(source_file, target)?;
    remove_source_path(source)
}

#[cfg(target_os = "macos")]
fn publish_new_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    let (guard, guard_file) = create_macos_publication_guard(source_file, target)?;
    let result = macos_rename_guard(&guard, target, libc::RENAME_EXCL);
    drop(guard_file);
    if result.is_err() {
        let _ = std::fs::remove_file(&guard);
    }
    result?;
    remove_source_path(source)
}

#[cfg(target_os = "macos")]
fn create_macos_publication_guard(
    source_file: &File,
    target: &Path,
) -> std::io::Result<(PathBuf, File)> {
    // macOS rejects descriptor links, so publish through a complete sibling guard.
    let file_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut guard_name = OsString::from(file_name);
        guard_name.push(format!(".publish.{}.{}", std::process::id(), sequence));
        let guard = parent.join(guard_name);
        match copy_held_file_to_new_path(source_file, &guard) {
            Ok(file) => return Ok((guard, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a macOS publication guard",
    ))
}

#[cfg(target_os = "macos")]
fn copy_held_file_to_new_path(source_file: &File, target: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let source_mode = source_file.metadata()?.permissions().mode() & 0o7777;
    let expected_len = source_file.metadata()?.len();
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true).mode(0o000);
    let mut destination = options.open(target)?;
    let result = (|| -> std::io::Result<()> {
        let mut held_source = source_file.try_clone()?;
        held_source.rewind()?;
        let copied = std::io::copy(&mut held_source, &mut destination)?;
        if copied != expected_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "held temporary file changed during macOS publication",
            ));
        }
        destination.sync_all()?;
        destination.set_permissions(std::fs::Permissions::from_mode(source_mode))?;
        destination.sync_all()
    })();
    if let Err(error) = result {
        drop(destination);
        let _ = std::fs::remove_file(target);
        return Err(error);
    }
    Ok(destination)
}

#[cfg(target_os = "macos")]
fn macos_rename_guard(guard: &Path, target: &Path, flags: libc::c_uint) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_file = File::open(parent)?;
    let guard_name = guard
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let target_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let guard_name = CString::new(guard_name.as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "publication guard contains a NUL byte",
        )
    })?;
    let target_name = CString::new(target_name.as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "publication target contains a NUL byte",
        )
    })?;
    let result = unsafe {
        libc::renameatx_np(
            parent_file.as_raw_fd(),
            guard_name.as_ptr(),
            parent_file.as_raw_fd(),
            target_name.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn create_descriptor_link(source_file: &File, target: &Path) -> std::io::Result<PathBuf> {
    let file_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut guard_name = OsString::from(file_name);
        guard_name.push(format!(".publish.{}.{sequence}", std::process::id()));
        let guard = parent.join(guard_name);
        match link_descriptor_to(source_file, &guard) {
            Ok(()) => return Ok(guard),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a descriptor publication link",
    ))
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "linux",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
fn link_descriptor_to(source_file: &File, target: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    const DESCRIPTOR_ROOT: &str = "/proc/self/fd";
    #[cfg(any(
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    const DESCRIPTOR_ROOT: &str = "/dev/fd";
    let descriptor_path = CString::new(format!("{DESCRIPTOR_ROOT}/{}", source_file.as_raw_fd()))
        .expect("descriptor path contains no NUL bytes");
    let target = CString::new(target.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target path contains a NUL byte",
        )
    })?;
    let result = unsafe {
        libc::linkat(
            libc::AT_FDCWD,
            descriptor_path.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "linux",
        target_os = "netbsd",
        target_os = "openbsd"
    ))
))]
fn link_descriptor_to(_source_file: &File, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "descriptor-bound atomic publication is unsupported on this Unix target",
    ))
}

#[cfg(windows)]
fn publish_new_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    rename_file_handle(source_file, target, false)?;
    remove_source_path(source)
}

#[cfg(windows)]
fn replace_validated_file(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    rename_file_handle(source_file, target, true)?;
    remove_source_path(source)
}

#[cfg(windows)]
fn rename_file_handle(
    source_file: &File,
    target: &Path,
    replace_existing: bool,
) -> std::io::Result<()> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
    };

    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()?.join(target)
    };
    let target = target.as_os_str().encode_wide().collect::<Vec<_>>();
    let buffer_bytes = size_of::<FILE_RENAME_INFO>() + target.len() * size_of::<u16>();
    let mut buffer = vec![0usize; buffer_bytes.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace_existing;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = u32::try_from(target.len() * size_of::<u16>())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        std::ptr::copy_nonoverlapping(target.as_ptr(), (*info).FileName.as_mut_ptr(), target.len());
    }
    let result = unsafe {
        SetFileInformationByHandle(
            source_file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            u32::try_from(buffer_bytes)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn publish_new_file(_source_file: &File, _source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "descriptor-bound atomic publication is unsupported on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn replace_validated_file(
    _source_file: &File,
    _source: &Path,
    _target: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "descriptor-bound atomic replacement is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn replace_file_by_path(_source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file_by_path(source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    rename_file_handle(source_file, target, true)?;
    remove_source_path(source)
}

#[cfg(not(any(unix, windows)))]
fn replace_file_by_path(_source_file: &File, source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

fn remove_source_path(source: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(source) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_parent(target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::OpenOptions::new()
            .write(true)
            .open(target)?
            .sync_all()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "zeff-atomic-file-{name}-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn overwrite_publishes_complete_new_bytes() {
        let root = TestDir::new("overwrite");
        let path = root.0.join("nested").join("save.sav");

        write_file_atomically(&path, b"old").unwrap();
        write_file_atomically(&path, b"complete new value").unwrap();

        assert_eq!(std::fs::read(path).unwrap(), b"complete new value");
    }

    #[test]
    fn injected_failures_leave_complete_old_or_new_primary() {
        const OLD: &[u8] = b"complete old value";
        const NEW: &[u8] = b"complete replacement value";
        let phases = [
            AtomicWritePhase::BeforeTemp,
            AtomicWritePhase::AfterTemp,
            AtomicWritePhase::AfterPartialWrite,
            AtomicWritePhase::AfterWrite,
            AtomicWritePhase::AfterSync,
            AtomicWritePhase::BeforeReplace,
            AtomicWritePhase::AfterReplace,
        ];

        for phase in phases {
            let root = TestDir::new(&format!("failure-{phase:?}"));
            let path = root.0.join("save.sav");
            std::fs::write(&path, OLD).unwrap();

            let result = write_file_atomically_with(
                &path,
                NEW,
                |_| Ok(()),
                replace_file_by_path,
                |current| {
                    if current == phase {
                        bail!("injected failure at {phase:?}");
                    }
                    Ok(())
                },
            );

            assert!(result.is_err());
            let primary = std::fs::read(&path).unwrap();
            let expected = if phase == AtomicWritePhase::AfterReplace {
                NEW
            } else {
                OLD
            };
            assert_eq!(primary, expected, "failure at {phase:?}");
            assert_no_staging_files(&root.0);
        }
    }

    #[test]
    fn replacement_failure_preserves_existing_primary() {
        let root = TestDir::new("replacement");
        let path = root.0.join("save.sav");
        std::fs::write(&path, b"complete old value").unwrap();

        let result = write_file_atomically_with(
            &path,
            b"complete new value",
            |_| Ok(()),
            |_, _, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected replacement failure",
                ))
            },
            |_| Ok(()),
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"complete old value");
        assert_no_staging_files(&root.0);
    }

    #[test]
    fn validation_failure_preserves_existing_primary() {
        let root = TestDir::new("validation");
        let path = root.0.join("project.ztas");
        std::fs::write(&path, b"valid old project").unwrap();

        let result = write_file_atomically_validated(&path, b"invalid new project", |_| {
            bail!("invalid project")
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"valid old project");
    }

    #[test]
    fn create_new_publication_never_replaces_existing_file() {
        let root = TestDir::new("create-new");
        let path = root.0.join("replay.zrpl");
        std::fs::write(&path, b"concurrent output").unwrap();

        let result = write_new_file_atomically_validated(&path, b"new replay", |_| Ok(()));

        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"concurrent output");
        assert_no_staging_files(&root.0);
    }

    #[test]
    fn create_new_publication_never_publishes_a_substituted_temp_path() {
        let root = TestDir::new("create-new-substitution");
        let path = root.0.join("replay.zrpl");

        let result = write_new_file_atomically_validated(&path, b"verified replay", |file| {
            let temp = only_temp_path(&root.0);
            std::fs::remove_file(&temp)?;
            std::fs::write(&temp, b"substituted replay")?;
            use std::io::{Read, Seek};
            file.rewind()?;
            let mut validated = Vec::new();
            file.read_to_end(&mut validated)?;
            assert_eq!(validated, b"verified replay");
            Ok(())
        });

        if result.is_ok() {
            assert_eq!(std::fs::read(&path).unwrap(), b"verified replay");
        } else {
            assert!(!path.exists());
        }
        assert_no_staging_files(&root.0);
    }

    #[test]
    fn overwrite_publication_never_publishes_a_substituted_temp_path() {
        let root = TestDir::new("overwrite-substitution");
        let path = root.0.join("project.ztas");
        std::fs::write(&path, b"old project").unwrap();

        let result = write_file_atomically_validated(&path, b"verified project", |file| {
            let temp = only_temp_path(&root.0);
            std::fs::remove_file(&temp)?;
            std::fs::write(&temp, b"substituted project")?;
            use std::io::{Read, Seek};
            file.rewind()?;
            let mut validated = Vec::new();
            file.read_to_end(&mut validated)?;
            assert_eq!(validated, b"verified project");
            Ok(())
        });

        if result.is_ok() {
            assert_eq!(std::fs::read(&path).unwrap(), b"verified project");
        } else {
            assert_eq!(std::fs::read(&path).unwrap(), b"old project");
        }
        assert_no_staging_files(&root.0);
    }

    #[test]
    fn create_new_publication_detects_in_place_mutation_at_checkpoint() {
        let root = TestDir::new("create-new-in-place-mutation");
        let path = root.0.join("replay.zrpl");
        let mut mutation_succeeded = false;

        let result = write_file_atomically_with(
            &path,
            b"verified replay",
            |_| Ok(()),
            publish_new_file,
            |phase| {
                if phase == AtomicWritePhase::BeforeReplace {
                    let temp = only_temp_path(&root.0);
                    if let Ok(mut writer) = std::fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(temp)
                    {
                        writer.write_all(b"mutated replay")?;
                        writer.sync_all()?;
                        mutation_succeeded = true;
                    }
                }
                Ok(())
            },
        );

        if mutation_succeeded {
            assert!(result.is_err());
            assert!(!path.exists());
        } else {
            result.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"verified replay");
        }
    }

    fn only_temp_path(root: &Path) -> PathBuf {
        std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".tmp.")
            })
            .expect("atomic writer should have one temporary file")
    }

    fn assert_no_staging_files(root: &Path) {
        assert!(std::fs::read_dir(root).unwrap().all(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            !name.contains(".tmp.") && !name.contains(".publish.")
        }));
    }
}
