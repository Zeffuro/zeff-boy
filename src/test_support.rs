use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_PATH: AtomicU64 = AtomicU64::new(1);

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

pub(crate) struct TestFile {
    path: PathBuf,
}

impl TestFile {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn test_directory(label: &str) -> io::Result<TestDirectory> {
    reserve_test_path(label, |path| std::fs::create_dir(path)).map(|path| TestDirectory { path })
}

pub(crate) fn test_file(label: &str) -> io::Result<TestFile> {
    reserve_test_path(label, |path| {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|_| ())
    })
    .map(|path| TestFile { path })
}

fn reserve_test_path(
    label: &str,
    reserve: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    for _ in 0..100 {
        let id = NEXT_TEST_PATH.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zeff-boy-test-{label}-{}-{id}", std::process::id()));

        match reserve(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not reserve a unique temporary test path for {label}"),
    ))
}

#[allow(dead_code)]
pub(crate) fn build_gb_test_rom() -> Vec<u8> {
    vec![0; 0x8000]
}

#[allow(dead_code)]
pub(crate) fn build_nes_test_rom() -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg..prg + 6].copy_from_slice(&[0xA9, 0x42, 0x85, 0x00, 0xEA, 0xEA]);
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_are_unique_and_cleaned_up() {
        let directory = test_directory("lifecycle").unwrap();
        let directory_path = directory.path().to_owned();
        let file = test_file("lifecycle").unwrap();
        let file_path = file.path().to_owned();

        assert_ne!(directory_path, file_path);
        assert!(directory_path.is_dir());
        assert!(file_path.is_file());

        drop(directory);
        drop(file);

        assert!(!directory_path.exists());
        assert!(!file_path.exists());
    }
}
