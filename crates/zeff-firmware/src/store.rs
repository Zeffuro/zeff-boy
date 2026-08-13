use std::path::Path;
use std::sync::Arc;

use crate::catalog::{FirmwareSpec, FirmwareVariantSpec};
use crate::digest::{DigestSet, hex_eq_digest, sha256_bytes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationStatus {
    KnownGood {
        spec_id: String,
        variant_id: String,
    },
    UnknownHash {
        spec_id: String,
        plausible_variant_ids: Vec<String>,
    },
    WrongSize {
        expected: Vec<String>,
        actual: u64,
    },
    NoMatchingSpec,
}

#[derive(Clone, Debug)]
pub struct FirmwareInventoryEntry {
    pub bytes: Arc<[u8]>,
    pub original_filename: Option<String>,
    pub digests: DigestSet,
    pub validation: ValidationStatus,
}

impl FirmwareInventoryEntry {
    pub fn from_bytes(
        bytes: impl Into<Arc<[u8]>>,
        original_filename: Option<String>,
        catalog: &[FirmwareSpec],
    ) -> Self {
        let bytes = bytes.into();
        let mut digests = DigestSet::from_bytes(&bytes);
        let validation = validate_entry(&bytes, original_filename.as_deref(), &digests, catalog);
        digests.sha256 = sha256_bytes(&bytes);
        Self {
            bytes,
            original_filename,
            digests,
            validation,
        }
    }

    pub fn with_legacy_digests(mut self, md5: Option<String>, sha1: Option<String>) -> Self {
        self.digests.md5 = md5;
        self.digests.sha1 = sha1;
        self
    }

    pub fn from_bytes_with_legacy_digests(
        bytes: impl Into<Arc<[u8]>>,
        original_filename: Option<String>,
        md5: Option<String>,
        sha1: Option<String>,
        catalog: &[FirmwareSpec],
    ) -> Self {
        let bytes = bytes.into();
        let digests = DigestSet {
            md5,
            sha1,
            sha256: sha256_bytes(&bytes),
        };
        let validation = validate_entry(&bytes, original_filename.as_deref(), &digests, catalog);
        Self {
            bytes,
            original_filename,
            digests,
            validation,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FirmwareInventory {
    entries: Vec<FirmwareInventoryEntry>,
}

impl FirmwareInventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: FirmwareInventoryEntry) {
        if let Some(existing) = self
            .entries
            .iter()
            .position(|existing| existing.digests.sha256 == entry.digests.sha256)
        {
            if validation_rank(&entry.validation)
                > validation_rank(&self.entries[existing].validation)
            {
                self.entries[existing] = entry;
            }
            return;
        }
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[FirmwareInventoryEntry] {
        &self.entries
    }

    pub fn from_directory(path: &Path, catalog: &[FirmwareSpec]) -> std::io::Result<Self> {
        let mut inventory = Self::new();
        let mut entries = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }

            let bytes = std::fs::read(entry.path())?;
            let original_filename = entry.file_name().to_str().map(str::to_owned);
            inventory.add(FirmwareInventoryEntry::from_bytes(
                bytes,
                original_filename,
                catalog,
            ));
        }

        Ok(inventory)
    }
}

fn validation_rank(validation: &ValidationStatus) -> u8 {
    match validation {
        ValidationStatus::KnownGood { .. } => 3,
        ValidationStatus::UnknownHash { .. } => 2,
        ValidationStatus::WrongSize { .. } => 1,
        ValidationStatus::NoMatchingSpec => 0,
    }
}

pub(crate) fn validate_entry(
    bytes: &[u8],
    original_filename: Option<&str>,
    digests: &DigestSet,
    catalog: &[FirmwareSpec],
) -> ValidationStatus {
    let len = bytes.len() as u64;
    let mut plausible = Vec::new();
    let mut wrong_size = Vec::new();

    for spec in catalog {
        for variant in spec.variants {
            let filename_relevant =
                original_filename.is_none_or(|filename| variant.filename_matches(filename));
            if !filename_relevant && !digest_matches_variant(digests, variant) {
                continue;
            }

            if !variant.size.matches(len) {
                wrong_size.push(format!("{}: {:?}", variant.id, variant.size));
                continue;
            }

            if digest_matches_variant(digests, variant) {
                return ValidationStatus::KnownGood {
                    spec_id: spec.id.to_owned(),
                    variant_id: variant.id.to_owned(),
                };
            }

            plausible.push((spec.id.to_owned(), variant.id.to_owned()));
        }
    }

    if !plausible.is_empty() {
        let spec_id = plausible[0].0.clone();
        return ValidationStatus::UnknownHash {
            spec_id,
            plausible_variant_ids: plausible
                .into_iter()
                .map(|(_, variant_id)| variant_id)
                .collect(),
        };
    }

    if !wrong_size.is_empty() {
        return ValidationStatus::WrongSize {
            expected: wrong_size,
            actual: len,
        };
    }

    ValidationStatus::NoMatchingSpec
}

pub(crate) fn digest_matches_variant(digests: &DigestSet, variant: &FirmwareVariantSpec) -> bool {
    if let Some(expected) = variant.hashes.sha256
        && hex_eq_digest(expected, &digests.sha256)
    {
        return true;
    }
    if let (Some(expected), Some(actual)) = (variant.hashes.md5, digests.md5.as_deref())
        && expected.eq_ignore_ascii_case(actual)
    {
        return true;
    }
    if let (Some(expected), Some(actual)) = (variant.hashes.sha1, digests.sha1.as_deref())
        && expected.eq_ignore_ascii_case(actual)
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FirmwareSpec, KnownHashes, SizeRule};

    const TEST_SPEC: FirmwareSpec = FirmwareSpec {
        id: "test.firmware",
        display_name: "Test Firmware",
        system: "Test",
        purpose: "Testing",
        variants: &[crate::catalog::FirmwareVariantSpec {
            id: "test.firmware.a",
            display_name: "Test Firmware A",
            region: "any",
            model: None,
            filenames: &["firmware.bin"],
            size: SizeRule::Exact(4),
            hashes: KnownHashes {
                md5: Some("test-md5"),
                sha1: None,
                sha256: None,
            },
        }],
    };

    #[test]
    fn correct_filename_wrong_hash_is_unknown_hash_not_known_good() {
        let entry = FirmwareInventoryEntry::from_bytes(
            b"abcd".as_slice(),
            Some("firmware.bin".into()),
            &[TEST_SPEC],
        );
        assert!(matches!(
            entry.validation,
            ValidationStatus::UnknownHash { .. }
        ));
    }

    #[test]
    fn matching_legacy_digest_identifies_known_good() {
        let entry = FirmwareInventoryEntry::from_bytes_with_legacy_digests(
            b"abcd".as_slice(),
            Some("whatever.rom".into()),
            Some("test-md5".to_owned()),
            None,
            &[TEST_SPEC],
        );
        assert!(matches!(
            entry.validation,
            ValidationStatus::KnownGood { .. }
        ));
    }

    #[test]
    fn inventory_deduplicates_by_sha256() {
        let mut inventory = FirmwareInventory::new();
        inventory.add(FirmwareInventoryEntry::from_bytes(
            b"abcd".as_slice(),
            Some("one.bin".into()),
            &[TEST_SPEC],
        ));
        inventory.add(FirmwareInventoryEntry::from_bytes(
            b"abcd".as_slice(),
            Some("two.bin".into()),
            &[TEST_SPEC],
        ));
        assert_eq!(inventory.entries().len(), 1);
    }

    #[test]
    fn inventory_from_directory_reads_regular_files_and_deduplicates() {
        let dir =
            std::env::temp_dir().join(format!("zeff_firmware_store_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("firmware.bin"), b"abcd").unwrap();
        std::fs::write(dir.join("duplicate.bin"), b"abcd").unwrap();
        std::fs::create_dir_all(dir.join("nested")).unwrap();

        let inventory = FirmwareInventory::from_directory(&dir, &[TEST_SPEC])
            .expect("firmware directory should scan");

        assert_eq!(inventory.entries().len(), 1);
        assert_eq!(
            inventory.entries()[0].original_filename.as_deref(),
            Some("firmware.bin")
        );
        assert!(matches!(
            inventory.entries()[0].validation,
            ValidationStatus::UnknownHash { .. }
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
