use std::fs;
use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};

use crate::model::TestCase;

pub(crate) enum HashCheck {
    Ok,
    NoExpectedHash,
    Mismatch { expected: String, actual: String },
}

pub(crate) fn verify_rom_hash(test: &TestCase, path: &Path) -> anyhow::Result<HashCheck> {
    let Some(expected) = &test.rom.sha256 else {
        return Ok(HashCheck::NoExpectedHash);
    };
    let actual = sha256_file(path)?;
    if expected.eq_ignore_ascii_case(&actual) {
        Ok(HashCheck::Ok)
    } else {
        Ok(HashCheck::Mismatch {
            expected: expected.clone(),
            actual,
        })
    }
}

pub(crate) fn verify_file_hash(path: &Path, expected: &str) -> anyhow::Result<HashCheck> {
    if expected.trim().is_empty() {
        return Ok(HashCheck::NoExpectedHash);
    }
    let actual = sha256_file(path)?;
    if expected.eq_ignore_ascii_case(&actual) {
        Ok(HashCheck::Ok)
    } else {
        Ok(HashCheck::Mismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

pub(crate) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn path_contains_component(path: &Path, component: &str) -> bool {
    path.components()
        .any(|part| part.as_os_str().to_string_lossy() == component)
}

pub(crate) fn path_starts_with<const N: usize>(path: &Path, components: [&str; N]) -> bool {
    let actual: Vec<String> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().replace('\\', "/"))
        .collect();
    actual.len() >= N
        && actual
            .iter()
            .zip(components)
            .all(|(actual, expected)| actual == expected)
}

pub(crate) fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}
