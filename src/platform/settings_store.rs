use std::io::{Read, Seek};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use super::{write_file_atomically_validated, write_new_file_atomically_validated};

pub(super) fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(
            String::from_utf8(bytes).context("settings are not UTF-8")?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn validate(file: &mut std::fs::File) -> Result<()> {
    file.rewind()?;
    let mut json = Vec::new();
    file.read_to_end(&mut json)?;
    let _: serde_json::Value = serde_json::from_slice(&json)?;
    Ok(())
}

fn acquire_write_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = path.with_extension("json.lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("cannot open settings writer lock {}", lock_path.display()))?;
    lock.try_lock().with_context(|| {
        format!(
            "settings are being saved by another instance ({})",
            lock_path.display()
        )
    })?;
    Ok(lock)
}

pub(super) fn save(
    path: &Path,
    json: &str,
    previous: Option<&str>,
    preserve_original: bool,
) -> Result<()> {
    save_with(path, json, previous, preserve_original, |path, bytes| {
        write_file_atomically_validated(path, bytes, validate)
    })
}

fn save_with(
    path: &Path,
    json: &str,
    previous: Option<&str>,
    preserve_original: bool,
    publish: impl FnOnce(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    let _: serde_json::Value = serde_json::from_str(json)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Hold the lease across stale-writer comparison, backup publication, and
    // primary replacement so cooperating processes cannot both pass the check.
    let _write_lock = acquire_write_lock(path)?;
    if !preserve_original && let Some(current) = read(path)? {
        if previous != Some(current.as_str()) {
            anyhow::bail!(
                "settings changed in another instance; restart before saving to keep those changes"
            );
        }
        if current == json {
            return Ok(());
        }
    }
    if preserve_original {
        match std::fs::read(path) {
            Ok(bytes) => {
                static RECOVERY_ID: AtomicU64 = AtomicU64::new(0);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = RECOVERY_ID.fetch_add(1, Ordering::Relaxed);
                let recovery = path.with_extension(format!(
                    "json.recovered-{timestamp}-{}-{id}",
                    std::process::id()
                ));
                write_new_file_atomically_validated(&recovery, &bytes, |_| Ok(()))
                    .context("could not preserve the original settings")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("could not preserve the original settings"),
        }
    }
    if let Some(previous) = previous {
        let value: serde_json::Value = serde_json::from_str(previous)?;
        // Keep the original flat document independently of the rolling backup,
        // so a second save does not destroy the migration rollback point.
        let legacy = path.with_extension("json.legacy");
        if value.get("schema_version").is_none() && !legacy.try_exists()? {
            write_new_file_atomically_validated(&legacy, previous.as_bytes(), validate)?;
        }
        write_file_atomically_validated(
            &path.with_extension("json.bak"),
            previous.as_bytes(),
            validate,
        )
        .context("could not update the settings backup")?;
    }
    publish(path, json.as_bytes()).context("could not replace the settings file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_followed_by_identical_autosave_preserves_the_previous_backup() {
        let dir = crate::test_support::test_directory("settings-import-autosave").unwrap();
        let path = dir.path().join("settings.json");
        let original = r#"{"schema_version":1,"original":true}"#;
        let imported = r#"{"schema_version":1,"imported":true}"#;
        std::fs::write(&path, original).unwrap();
        save(&path, imported, Some(original), false).unwrap();
        save_with(&path, imported, Some(imported), false, |_, _| {
            panic!("an identical document must not be republished")
        })
        .unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(imported));
        assert_eq!(
            read(&path.with_extension("json.bak")).unwrap().as_deref(),
            Some(original)
        );
        assert!(save(&path, imported, Some(original), false).is_err());
    }

    #[test]
    fn missing_primary_is_recreated_even_when_it_matches_the_recovery_document() {
        let dir = crate::test_support::test_directory("settings-missing-primary").unwrap();
        let path = dir.path().join("settings.json");
        let recovered = r#"{"schema_version":1}"#;
        save(&path, recovered, Some(recovered), false).unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(recovered));
    }

    #[test]
    fn publication_failure_preserves_prior_primary_and_valid_backup() {
        let dir = crate::test_support::test_directory("settings-publish-failure").unwrap();
        let path = dir.path().join("settings.json");
        let prior = r#"{"master_volume":0.25}"#;
        std::fs::write(&path, prior).unwrap();
        let result = save_with(
            &path,
            r#"{"schema_version":1}"#,
            Some(prior),
            false,
            |_, _| anyhow::bail!("injected replacement failure"),
        );
        assert!(result.is_err());
        assert_eq!(read(&path).unwrap().as_deref(), Some(prior));
        assert_eq!(
            read(&path.with_extension("json.bak")).unwrap().as_deref(),
            Some(prior)
        );
        assert_eq!(
            read(&path.with_extension("json.legacy"))
                .unwrap()
                .as_deref(),
            Some(prior)
        );
    }

    #[test]
    fn corrupt_original_is_preserved_before_recovery_is_published() {
        let dir = crate::test_support::test_directory("settings-recover-original").unwrap();
        let path = dir.path().join("settings.json");
        let original = b"{broken\xff";
        std::fs::write(&path, original).unwrap();
        save(&path, r#"{"schema_version":1}"#, Some("{}"), true).unwrap();
        let recovered = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".recovered-")
            })
            .unwrap();
        assert_eq!(std::fs::read(recovered).unwrap(), original);
        assert_eq!(
            read(&path).unwrap().as_deref(),
            Some(r#"{"schema_version":1}"#)
        );
    }

    #[test]
    fn repeated_saves_retain_the_pre_migration_document() {
        let dir = crate::test_support::test_directory("settings-legacy-backup").unwrap();
        let path = dir.path().join("settings.json");
        save(
            &path,
            r#"{"schema_version":1}"#,
            Some(r#"{"master_volume":0.4}"#),
            false,
        )
        .unwrap();
        save(
            &path,
            r#"{"schema_version":1,"second":true}"#,
            Some(r#"{"schema_version":1}"#),
            false,
        )
        .unwrap();
        assert_eq!(
            read(&path.with_extension("json.legacy"))
                .unwrap()
                .as_deref(),
            Some(r#"{"master_volume":0.4}"#)
        );
        assert_eq!(
            read(&path.with_extension("json.bak")).unwrap().as_deref(),
            Some(r#"{"schema_version":1}"#)
        );
    }

    #[test]
    fn another_instances_document_is_not_overwritten() {
        let dir = crate::test_support::test_directory("settings-concurrent-edit").unwrap();
        let path = dir.path().join("settings.json");
        let newer = r#"{"schema_version":999,"new_setting":true}"#;
        std::fs::write(&path, newer).unwrap();
        assert!(save(&path, "{}", Some(r#"{"schema_version":1}"#), false).is_err());
        assert_eq!(read(&path).unwrap().as_deref(), Some(newer));
        assert!(!path.with_extension("json.bak").exists());
    }

    #[test]
    fn second_cooperating_writer_cannot_publish_while_lease_is_held() {
        let dir = crate::test_support::test_directory("settings-writer-lock").unwrap();
        let path = dir.path().join("settings.json");
        let prior = r#"{"schema_version":1}"#;
        std::fs::write(&path, prior).unwrap();
        let _lease = acquire_write_lock(&path).unwrap();

        let error = save(
            &path,
            r#"{"schema_version":1,"changed":true}"#,
            Some(prior),
            false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("another instance"));
        assert_eq!(read(&path).unwrap().as_deref(), Some(prior));
        assert!(!path.with_extension("json.bak").exists());
    }
}
