use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

/// A unique temporary directory that is removed when its test finishes.
///
/// Headless integration tests use on-disk ROMs, states, and replays. Keeping
/// their directory lifecycle here makes that boundary consistent and avoids
/// time-based names that can collide in parallel test runs.
pub(crate) struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn test_directory(label: &str) -> io::Result<TestDirectory> {
    for _ in 0..100 {
        let id = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zeff-boy-test-{label}-{}-{id}", std::process::id()));

        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(TestDirectory { path }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not reserve a unique temporary test directory for {label}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directories_are_unique_and_cleaned_up() {
        let first = test_directory("lifecycle").unwrap();
        let first_path = first.path().to_owned();
        let second = test_directory("lifecycle").unwrap();
        let second_path = second.path().to_owned();

        assert_ne!(first_path, second_path);
        assert!(first_path.is_dir());
        assert!(second_path.is_dir());

        drop(first);
        drop(second);

        assert!(!first_path.exists());
        assert!(!second_path.exists());
    }
}
