use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(crate) struct NativeFirmwareImport {
    pub(crate) spec_id: String,
    pub(crate) variant_id: String,
    pub(crate) destination: PathBuf,
}

pub(crate) fn managed_firmware_dir() -> PathBuf {
    super::settings_dir().join("firmware")
}

pub(crate) fn import_firmware_file(source: &Path) -> anyhow::Result<NativeFirmwareImport> {
    let bytes = std::fs::read(source)
        .with_context(|| format!("Failed to read firmware file {}", source.display()))?;
    let original_filename = source
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let catalog = zeff_firmware::catalog_specs();
    let entry = zeff_firmware::FirmwareInventoryEntry::from_bytes(
        bytes.clone(),
        original_filename,
        catalog,
    );
    let (spec_id, variant_id) = match &entry.validation {
        zeff_firmware::ValidationStatus::KnownGood {
            spec_id,
            variant_id,
        } => (spec_id.clone(), variant_id.clone()),
        zeff_firmware::ValidationStatus::UnknownHash { .. } => {
            anyhow::bail!(
                "The selected file has a recognized name or size, but its hash is unknown"
            )
        }
        zeff_firmware::ValidationStatus::WrongSize { expected, actual } => {
            anyhow::bail!(
                "The selected file is {actual} bytes; expected {}",
                expected.join(" or ")
            )
        }
        zeff_firmware::ValidationStatus::NoMatchingSpec => {
            anyhow::bail!("The selected file is not recognized firmware")
        }
    };
    let filename = canonical_filename(catalog, &spec_id, &variant_id)?;
    let destination = managed_firmware_dir().join(filename);
    publish_atomic(&destination, &bytes)?;

    Ok(NativeFirmwareImport {
        spec_id,
        variant_id,
        destination,
    })
}

pub(crate) fn remove_managed_firmware(filename: &str) -> anyhow::Result<()> {
    let catalog = zeff_firmware::catalog_specs();
    let is_canonical = catalog.iter().any(|spec| {
        spec.variants
            .iter()
            .any(|variant| variant.filenames.first().copied() == Some(filename))
    });
    if !is_canonical
        || Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(filename)
    {
        anyhow::bail!("firmware removal key is not a canonical managed filename");
    }

    let destination = managed_firmware_dir().join(filename);
    let bytes = std::fs::read(&destination)
        .with_context(|| format!("Failed to read managed firmware {}", destination.display()))?;
    let entry = zeff_firmware::FirmwareInventoryEntry::from_bytes(
        bytes,
        Some(filename.to_owned()),
        catalog,
    );
    if !matches!(
        entry.validation,
        zeff_firmware::ValidationStatus::KnownGood { .. }
    ) {
        anyhow::bail!(
            "Managed firmware {} no longer matches the recognized catalog entry",
            destination.display()
        );
    }
    std::fs::remove_file(&destination).with_context(|| {
        format!(
            "Failed to remove managed firmware {}",
            destination.display()
        )
    })?;
    Ok(())
}

fn canonical_filename<'a>(
    catalog: &'a [zeff_firmware::FirmwareSpec],
    spec_id: &str,
    variant_id: &str,
) -> anyhow::Result<&'a str> {
    let spec = catalog
        .iter()
        .find(|spec| spec.id == spec_id)
        .ok_or_else(|| anyhow::anyhow!("recognized firmware spec is missing from the catalog"))?;
    let variant = spec.variant(variant_id).ok_or_else(|| {
        anyhow::anyhow!("recognized firmware variant is missing from the catalog")
    })?;
    let filename =
        variant.filenames.first().copied().ok_or_else(|| {
            anyhow::anyhow!("recognized firmware variant has no canonical filename")
        })?;
    let path = Path::new(filename);
    if path.file_name().and_then(|name| name.to_str()) != Some(filename) {
        anyhow::bail!("firmware catalog canonical filename is not a plain filename");
    }
    Ok(filename)
}

fn publish_atomic(destination: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("managed firmware path has no parent"))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create firmware directory {}", parent.display()))?;

    if destination.exists() {
        return verify_existing(destination, bytes);
    }

    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".zeff-firmware-{}-{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("Failed to create temporary file {}", temp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("Failed to write temporary file {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to flush temporary file {}", temp.display()))?;
        drop(file);

        match std::fs::hard_link(&temp, destination) {
            Ok(()) => {
                sync_directory(parent);
                Ok(())
            }
            Err(_) if destination.exists() => verify_existing(destination, bytes),
            Err(error) => Err(error).with_context(|| {
                format!(
                    "Failed to publish managed firmware {}",
                    destination.display()
                )
            }),
        }
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

fn verify_existing(destination: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let existing = std::fs::read(destination).with_context(|| {
        format!(
            "Failed to read existing managed firmware {}",
            destination.display()
        )
    })?;
    if existing == bytes {
        Ok(())
    } else {
        anyhow::bail!(
            "Managed firmware {} already exists with different contents",
            destination.display()
        )
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) {
    if let Ok(directory) = std::fs::File::open(path) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zeff_managed_firmware_{name}_{}_{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn canonical_filenames_resolve_from_catalog_variants() {
        let catalog = zeff_firmware::catalog_specs();
        assert_eq!(
            canonical_filename(catalog, "nintendo.gba.bios", "nintendo.gba.bios.agb").unwrap(),
            "gba_bios.bin"
        );
        assert_eq!(
            canonical_filename(catalog, "sega.sms.boot", "sega.sms.boot.japan").unwrap(),
            "bios_J.sms"
        );
    }

    #[test]
    fn atomic_publish_is_idempotent_and_never_clobbers() {
        let root = temp_dir("atomic");
        let destination = root.join("gba_bios.bin");
        publish_atomic(&destination, b"known firmware").unwrap();
        publish_atomic(&destination, b"known firmware").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"known firmware");

        let error = publish_atomic(&destination, b"different firmware").unwrap_err();
        assert!(error.to_string().contains("different contents"));
        assert_eq!(std::fs::read(&destination).unwrap(), b"known firmware");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_firmware_is_rejected_before_managed_path_creation() {
        let root = temp_dir("unknown");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("not-firmware.bin");
        std::fs::write(&source, b"not firmware").unwrap();

        let error = import_firmware_file(&source).unwrap_err();
        assert!(error.to_string().contains("not recognized firmware"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn removal_rejects_paths_and_unrecognized_contents() {
        assert!(remove_managed_firmware("../gba_bios.bin").is_err());
        assert!(remove_managed_firmware("not-catalog.bin").is_err());
    }
}
