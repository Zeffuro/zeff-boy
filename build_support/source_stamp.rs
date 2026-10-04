use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const ROOT_FILES: &[&str] = &["Cargo.toml", "Cargo.lock", "build.rs"];
const INPUT_DIRS: &[&str] = &[
    "src",
    "crates",
    "third_party",
    "tools",
    "build_support",
    "assets",
    ".cargo",
];
const TABLE: &str = "src/netplay/qualification.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStamp {
    pub full: String,
    pub qualification: String,
    pub file_count: usize,
    pub inputs: Vec<PathBuf>,
}

pub fn snapshot(root: &Path) -> io::Result<SourceStamp> {
    let root = root.canonicalize()?;
    let mut entries = Vec::new();
    let mut inputs = Vec::new();
    for name in ROOT_FILES {
        collect(&root, &root.join(name), &mut entries, &mut inputs)?;
    }
    for name in INPUT_DIRS {
        let path = root.join(name);
        inputs.push(path.clone());
        match fs::symlink_metadata(&path) {
            Ok(_) => collect(&root, &path, &mut entries, &mut inputs)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    inputs.sort();
    inputs.dedup();
    let mut full = Sha256::new();
    let mut qualification = Sha256::new();
    let mut file_count = 0;
    for (name, bytes) in entries {
        file_count += usize::from(bytes.is_some());
        append(&mut full, &name, bytes.as_deref());
        if name != TABLE {
            append(&mut qualification, &name, bytes.as_deref());
        }
    }
    Ok(SourceStamp {
        full: full
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        qualification: qualification
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        file_count,
        inputs,
    })
}

fn collect(
    root: &Path,
    path: &Path,
    entries: &mut Vec<(String, Option<Vec<u8>>)>,
    inputs: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "source input is a symlink: {}",
            path.display()
        )));
    }
    let name = path.strip_prefix(root).map_err(io::Error::other)?;
    let name = name
        .to_str()
        .ok_or_else(|| io::Error::other("non-UTF-8 source path"))?
        .replace('\\', "/");
    // The binding compiles vendor/, not the ignored upstream checkout.
    if name == "third_party/xdelta3/xdelta3" {
        return Ok(());
    }
    inputs.push(path.to_path_buf());
    if metadata.is_file() {
        entries.push((name, Some(fs::read(path)?)));
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if matches!(
                entry.file_name().to_str(),
                Some("target" | ".git" | ".dev_docs" | ".tmp")
            ) {
                continue;
            }
            collect(root, &entry.path(), entries, inputs)?;
        }
    } else {
        return Err(io::Error::other(format!(
            "unsupported source input: {}",
            path.display()
        )));
    }
    Ok(())
}

fn append(hash: &mut Sha256, name: &str, bytes: Option<&[u8]>) {
    hash.update((name.len() as u64).to_le_bytes());
    hash.update(name.as_bytes());
    hash.update([u8::from(bytes.is_some())]);
    if let Some(bytes) = bytes {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zeff-source-stamp-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(path.join("src/netplay")).unwrap();
            for name in ROOT_FILES {
                fs::write(path.join(name), name).unwrap();
            }
            fs::write(path.join("src/netplay/compatibility.rs"), b"parser").unwrap();
            fs::write(path.join(TABLE), b"[]").unwrap();
            Self(path)
        }
        fn stamp(&self) -> SourceStamp {
            snapshot(&self.0).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn content_edits_additions_and_deletions_change_both_stamps() {
        let fixture = Fixture::new();
        let original = fixture.stamp();
        let source = fixture.0.join("src/netplay/compatibility.rs");
        fs::write(&source, b"new parser").unwrap();
        let edited = fixture.stamp();
        assert_ne!(original.full, edited.full);
        assert_ne!(original.qualification, edited.qualification);
        fs::write(&source, b"parser").unwrap();
        let added = fixture.0.join("src/untracked.rs");
        fs::write(&added, b"production input").unwrap();
        assert_ne!(original.qualification, fixture.stamp().qualification);
        fs::remove_file(&added).unwrap();
        assert_eq!(original, fixture.stamp());
        fs::remove_file(&source).unwrap();
        assert_ne!(original.qualification, fixture.stamp().qualification);
    }

    #[test]
    fn location_and_private_outputs_do_not_change_stamp() {
        let left = Fixture::new();
        let right = Fixture::new();
        assert_eq!(left.stamp().full, right.stamp().full);
        for dir in [".tmp", ".dev_docs", "target"] {
            fs::create_dir_all(left.0.join(dir)).unwrap();
            fs::write(left.0.join(dir).join("capture.rs"), b"private").unwrap();
        }
        assert_eq!(left.stamp().qualification, right.stamp().qualification);
        assert!(
            !left
                .stamp()
                .inputs
                .contains(&left.0.canonicalize().unwrap())
        );
    }

    #[test]
    fn only_table_data_is_excluded_from_qualification() {
        let fixture = Fixture::new();
        let before = fixture.stamp();
        fs::write(fixture.0.join(TABLE), b"[1]").unwrap();
        let after = fixture.stamp();
        assert_ne!(before.full, after.full);
        assert_eq!(before.qualification, after.qualification);
        fs::write(
            fixture.0.join("src/netplay/compatibility.rs"),
            b"new parser",
        )
        .unwrap();
        assert_ne!(after.qualification, fixture.stamp().qualification);
    }

    #[test]
    fn unused_upstream_checkout_is_excluded_but_compiled_vendor_is_bound() {
        let fixture = Fixture::new();
        let vendor = fixture.0.join("third_party/xdelta3/vendor");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("xdelta3.c"), b"compiled").unwrap();
        let before = fixture.stamp();
        let unused = fixture.0.join("third_party/xdelta3/xdelta3");
        fs::create_dir_all(&unused).unwrap();
        fs::write(unused.join("xdelta3.c"), b"unused").unwrap();
        assert_eq!(before, fixture.stamp());
        fs::write(vendor.join("xdelta3.c"), b"changed").unwrap();
        assert_ne!(before.qualification, fixture.stamp().qualification);
    }

    #[test]
    fn empty_directory_layout_is_irrelevant_but_required_root_inputs_must_exist() {
        let fixture = Fixture::new();
        let before = fixture.stamp();
        for name in ["src/unused", "tools/empty", ".cargo/empty"] {
            fs::create_dir_all(fixture.0.join(name)).unwrap();
        }
        let after = fixture.stamp();
        assert_eq!(before.full, after.full);
        assert_eq!(before.qualification, after.qualification);
        assert_eq!(before.file_count, after.file_count);
        fs::remove_file(fixture.0.join("Cargo.lock")).unwrap();
        assert!(snapshot(&fixture.0).is_err());
    }
}
