use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

const MAX_HISTORY: usize = 16;
const MAX_PENDING_HISTORY: usize = 16;
const MAX_DIRECTORY_ENTRIES: usize = 256;
const MAX_SRAM_BYTES: u64 = 16 * 1024 * 1024;
static UPDATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RecoveryKey {
    directory: PathBuf,
}

struct RecoveryEntry {
    initial_primary: InitialPrimary,
    session_start_written: bool,
    pending: VecDeque<Vec<u8>>,
}

impl Default for RecoveryEntry {
    fn default() -> Self {
        Self {
            initial_primary: InitialPrimary::Uninitialized,
            session_start_written: false,
            pending: VecDeque::new(),
        }
    }
}

enum InitialPrimary {
    Uninitialized,
    Captured(Option<Vec<u8>>),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteKind {
    Primary,
    SessionStart,
    History,
    Prune,
}

pub(crate) struct RecoverySession {
    entries: HashMap<RecoveryKey, RecoveryEntry>,
    enabled: bool,
}

impl Default for RecoverySession {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            enabled: true,
        }
    }
}

impl RecoverySession {
    #[cfg(test)]
    pub(crate) fn authoritative_only() -> Self {
        Self {
            entries: HashMap::new(),
            enabled: false,
        }
    }

    pub(crate) fn begin(
        &mut self,
        primary_path: &Path,
        system_subdir: &str,
        media_identity: [u8; 32],
        component: &str,
    ) {
        if !self.enabled {
            return;
        }
        if let Ok(directory) = recovery_directory(system_subdir, media_identity, component) {
            self.begin_with(primary_path, directory);
        }
    }

    fn begin_with(&mut self, primary_path: &Path, recovery_directory: PathBuf) {
        let entry = self
            .entries
            .entry(RecoveryKey {
                directory: recovery_directory,
            })
            .or_default();
        if matches!(entry.initial_primary, InitialPrimary::Uninitialized) {
            entry.initial_primary = match read_bounded(primary_path) {
                Ok(bytes) => InitialPrimary::Captured(bytes),
                Err(error) => InitialPrimary::Failed(error.to_string()),
            };
        }
    }

    pub(crate) fn write(
        &mut self,
        primary_path: &Path,
        system_subdir: &str,
        media_identity: [u8; 32],
        component: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if !self.enabled {
            if bytes.len() as u64 > MAX_SRAM_BYTES {
                bail!("save data exceeds the {MAX_SRAM_BYTES}-byte recovery limit");
            }
            return crate::platform::write_save_data(primary_path, bytes);
        }
        let directory = recovery_directory(system_subdir, media_identity, component)?;
        self.write_with(primary_path, directory, bytes, |_| Ok(()))
    }

    fn write_with(
        &mut self,
        primary_path: &Path,
        recovery_directory: PathBuf,
        bytes: &[u8],
        checkpoint: impl FnMut(WriteKind) -> Result<()>,
    ) -> Result<()> {
        let _guard = UPDATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.write_locked(primary_path, recovery_directory, bytes, checkpoint)
    }

    fn write_locked(
        &mut self,
        primary_path: &Path,
        recovery_directory: PathBuf,
        bytes: &[u8],
        mut checkpoint: impl FnMut(WriteKind) -> Result<()>,
    ) -> Result<()> {
        if bytes.len() as u64 > MAX_SRAM_BYTES {
            bail!("save data exceeds the {MAX_SRAM_BYTES}-byte recovery limit");
        }
        let mut primary = read_bounded(primary_path)
            .with_context(|| format!("failed to read save data: {}", primary_path.display()))?;
        let key = RecoveryKey {
            directory: recovery_directory.clone(),
        };
        let entry = self.entries.entry(key).or_default();
        match &entry.initial_primary {
            InitialPrimary::Uninitialized => bail!("save recovery session was not initialized"),
            InitialPrimary::Failed(error) => {
                bail!("save recovery session could not read its initial primary: {error}")
            }
            InitialPrimary::Captured(_) => {}
        }

        if let Err(error) = drain_pending(&recovery_directory, entry, &mut checkpoint) {
            log::warn!("failed to update passive save recovery: {error:#}");
        }

        if primary.as_deref() == Some(bytes) {
            return Ok(());
        }

        let previous = primary.take();
        checkpoint(WriteKind::Primary)?;
        if let Err(error) = crate::platform::write_save_data(primary_path, bytes) {
            let published = read_bounded(primary_path)
                .with_context(|| {
                    format!(
                        "failed to reconcile save data after write error: {}",
                        primary_path.display()
                    )
                })?
                .is_some_and(|current| current == bytes);
            if !published {
                return Err(error).with_context(|| {
                    format!("failed to write save data: {}", primary_path.display())
                });
            }
            if let Some(previous) = previous {
                push_pending(entry, previous);
            }
            return Err(error).with_context(|| {
                format!(
                    "save data was published but final durability sync failed: {}",
                    primary_path.display()
                )
            });
        }

        if let Some(previous) = previous {
            push_pending(entry, previous);
            if let Err(error) = drain_pending(&recovery_directory, entry, &mut checkpoint) {
                log::warn!("save data committed, but passive recovery failed: {error:#}");
            }
        }
        Ok(())
    }
}

fn push_pending(entry: &mut RecoveryEntry, previous: Vec<u8>) {
    if entry
        .pending
        .back()
        .is_some_and(|pending| pending == &previous)
    {
        return;
    }
    if entry.pending.len() == MAX_PENDING_HISTORY {
        entry.pending.pop_front();
    }
    entry.pending.push_back(previous);
}

fn drain_pending(
    recovery_directory: &Path,
    entry: &mut RecoveryEntry,
    checkpoint: &mut impl FnMut(WriteKind) -> Result<()>,
) -> Result<()> {
    while let Some(previous) = entry.pending.pop_front() {
        if let Err(error) = persist_recovery(recovery_directory, entry, &previous, checkpoint) {
            entry.pending.push_front(previous);
            return Err(error);
        }
    }
    Ok(())
}

fn persist_recovery(
    recovery_directory: &Path,
    entry: &mut RecoveryEntry,
    previous: &[u8],
    checkpoint: &mut impl FnMut(WriteKind) -> Result<()>,
) -> Result<()> {
    if !entry.session_start_written {
        if let InitialPrimary::Captured(Some(initial_primary)) = &entry.initial_primary {
            checkpoint(WriteKind::SessionStart)?;
            crate::platform::write_save_data(
                &recovery_directory.join("session-start.sav"),
                initial_primary,
            )?;
        }
        entry.session_start_written = true;
    }

    let history_directory = recovery_directory.join("history");
    let previous_hash = zeff_firmware::sha256_bytes(previous);
    let mut history = read_history(&history_directory)?;
    let duplicate = history.last().is_some_and(|entry| {
        entry.hash == previous_hash
            && std::fs::metadata(&entry.path)
                .ok()
                .is_some_and(|metadata| metadata.len() == previous.len() as u64)
            && bounded_file_hash(&entry.path, previous.len()) == Some(previous_hash)
    });
    if !duplicate {
        let generation = history.last().map_or(Ok(1), |entry| {
            entry
                .generation
                .checked_add(1)
                .context("save recovery generation overflow")
        })?;
        checkpoint(WriteKind::History)?;
        let path = history_directory.join(format!(
            "{generation:016X}_{}.sav",
            const_hex::encode(previous_hash)
        ));
        crate::platform::write_save_data(&path, previous)?;
        history.push(HistoryEntry {
            generation,
            hash: previous_hash,
            path,
        });
    }

    history.sort_by_key(|entry| entry.generation);
    let prune_count = history.len().saturating_sub(MAX_HISTORY);
    for entry in history.into_iter().take(prune_count) {
        checkpoint(WriteKind::Prune)?;
        std::fs::remove_file(&entry.path)
            .with_context(|| format!("failed to prune save recovery {}", entry.path.display()))?;
    }
    Ok(())
}

fn bounded_file_hash(path: &Path, expected_len: usize) -> Option<[u8; 32]> {
    let bytes = read_bounded(path).ok()??;
    (bytes.len() == expected_len).then(|| zeff_firmware::sha256_bytes(&bytes))
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SRAM_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SRAM_BYTES {
        bail!("save data exceeds the {MAX_SRAM_BYTES}-byte recovery limit");
    }
    Ok(Some(bytes))
}

fn recovery_directory(
    system_subdir: &str,
    media_identity: [u8; 32],
    component: &str,
) -> Result<PathBuf> {
    validate_component(system_subdir)?;
    validate_component(component)?;
    Ok(crate::platform::save_dir(system_subdir)
        .join("recovery")
        .join(const_hex::encode(media_identity))
        .join(component))
}

struct HistoryEntry {
    generation: u64,
    hash: [u8; 32],
    path: PathBuf,
}

fn read_history(directory: &Path) -> Result<Vec<HistoryEntry>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut history = Vec::new();
    for (index, entry) in std::fs::read_dir(directory)?.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            bail!("save recovery history contains too many entries");
        }
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some((generation, hash)) = parse_history_name(&entry.file_name().to_string_lossy())
        else {
            continue;
        };
        history.push(HistoryEntry {
            generation,
            hash,
            path: entry.path(),
        });
    }
    history.sort_by_key(|entry| entry.generation);
    Ok(history)
}

fn parse_history_name(name: &str) -> Option<(u64, [u8; 32])> {
    let stem = name.strip_suffix(".sav")?;
    let (generation, hash) = stem.split_once('_')?;
    if generation.len() != 16 || hash.len() != 64 {
        return None;
    }
    let generation = u64::from_str_radix(generation, 16).ok()?;
    let hash = const_hex::decode_to_array(hash).ok()?;
    Some((generation, hash))
}

fn validate_component(component: &str) -> Result<()> {
    if component.is_empty()
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid save recovery path component")
    }
    Ok(())
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
                "zeff-sram-recovery-{name}-{}-{id}",
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

    fn history_bytes(directory: &Path) -> Vec<Vec<u8>> {
        let mut entries = read_history(&directory.join("history")).unwrap();
        entries.sort_by_key(|entry| entry.generation);
        entries
            .into_iter()
            .map(|entry| std::fs::read(entry.path).unwrap())
            .collect()
    }

    #[test]
    fn unchanged_and_first_created_primary_make_no_recovery_copy() {
        let root = TestDir::new("unchanged");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();

        std::fs::write(&primary, b"same").unwrap();
        session.begin_with(&primary, recovery.clone());
        session
            .write_with(&primary, recovery.clone(), b"same", |_| Ok(()))
            .unwrap();
        assert!(!recovery.exists());

        let new_primary = root.0.join("new.sav");
        let new_recovery = root.0.join("new-recovery");
        session.begin_with(&new_primary, new_recovery.clone());
        session
            .write_with(&new_primary, new_recovery.clone(), b"created", |_| Ok(()))
            .unwrap();
        assert_eq!(std::fs::read(new_primary).unwrap(), b"created");
        assert!(!new_recovery.exists());
    }

    #[test]
    fn first_dirty_commit_preserves_session_start_and_history() {
        let root = TestDir::new("session-start");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, b"start").unwrap();
        session.begin_with(&primary, recovery.clone());

        session
            .write_with(&primary, recovery.clone(), b"second", |_| Ok(()))
            .unwrap();
        session
            .write_with(&primary, recovery.clone(), b"third", |_| Ok(()))
            .unwrap();

        assert_eq!(std::fs::read(primary).unwrap(), b"third");
        assert_eq!(
            std::fs::read(recovery.join("session-start.sav")).unwrap(),
            b"start"
        );
        assert_eq!(
            history_bytes(&recovery),
            [b"start".to_vec(), b"second".to_vec()]
        );
    }

    #[test]
    fn history_retains_nonconsecutive_repeats_and_prunes_to_sixteen() {
        let root = TestDir::new("retention");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, [0]).unwrap();
        session.begin_with(&primary, recovery.clone());

        for value in 1..=18 {
            session
                .write_with(&primary, recovery.clone(), &[value], |_| Ok(()))
                .unwrap();
        }
        session
            .write_with(&primary, recovery.clone(), &[17], |_| Ok(()))
            .unwrap();
        session
            .write_with(&primary, recovery.clone(), &[19], |_| Ok(()))
            .unwrap();

        let history = history_bytes(&recovery);
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history.last().unwrap(), &[17]);
        assert_eq!(
            history
                .iter()
                .filter(|bytes| bytes.as_slice() == [17])
                .count(),
            2
        );
    }

    #[test]
    fn consecutive_history_duplicates_are_suppressed() {
        let root = TestDir::new("dedup");
        let recovery = root.0.join("recovery");
        let mut entry = RecoveryEntry {
            initial_primary: InitialPrimary::Captured(Some(b"old".to_vec())),
            ..Default::default()
        };

        persist_recovery(&recovery, &mut entry, b"old", &mut |_| Ok(())).unwrap();
        persist_recovery(&recovery, &mut entry, b"old", &mut |_| Ok(())).unwrap();

        assert_eq!(history_bytes(&recovery), [b"old"]);
    }

    #[test]
    fn primary_failure_does_not_advance_recovery() {
        let root = TestDir::new("primary-failure");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, b"old").unwrap();
        session.begin_with(&primary, recovery.clone());

        let result = session.write_with(&primary, recovery.clone(), b"new", |kind| {
            if kind == WriteKind::Primary {
                bail!("injected primary failure");
            }
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read(primary).unwrap(), b"old");
        assert!(!recovery.exists());
    }

    #[test]
    fn recovery_failure_retries_after_primary_commit() {
        let root = TestDir::new("recovery-retry");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, b"old").unwrap();
        session.begin_with(&primary, recovery.clone());

        session
            .write_with(&primary, recovery.clone(), b"new", |kind| {
                if kind == WriteKind::History {
                    bail!("injected history failure");
                }
                Ok(())
            })
            .unwrap();
        session
            .write_with(&primary, recovery.clone(), b"newer", |kind| {
                if kind == WriteKind::History {
                    bail!("injected history failure");
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(std::fs::read(&primary).unwrap(), b"newer");

        session
            .write_with(&primary, recovery.clone(), b"newer", |_| Ok(()))
            .unwrap();
        assert_eq!(history_bytes(&recovery), [b"old", b"new"]);
    }

    #[test]
    fn session_start_failure_retries_after_primary_commit() {
        let root = TestDir::new("session-retry");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, b"old").unwrap();
        session.begin_with(&primary, recovery.clone());

        session
            .write_with(&primary, recovery.clone(), b"new", |kind| {
                if kind == WriteKind::SessionStart {
                    bail!("injected session-start failure");
                }
                Ok(())
            })
            .unwrap();

        session
            .write_with(&primary, recovery.clone(), b"new", |_| Ok(()))
            .unwrap();
        assert_eq!(
            std::fs::read(recovery.join("session-start.sav")).unwrap(),
            b"old"
        );
        assert_eq!(history_bytes(&recovery), [b"old"]);
    }

    #[test]
    fn prune_failure_retries_without_duplicating_history() {
        let root = TestDir::new("prune-retry");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        std::fs::write(&primary, [0]).unwrap();
        session.begin_with(&primary, recovery.clone());
        for value in 1..=MAX_HISTORY as u8 {
            session
                .write_with(&primary, recovery.clone(), &[value], |_| Ok(()))
                .unwrap();
        }

        session
            .write_with(&primary, recovery.clone(), b"overflow", |kind| {
                if kind == WriteKind::Prune {
                    bail!("injected prune failure");
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(history_bytes(&recovery).len(), MAX_HISTORY + 1);

        session
            .write_with(&primary, recovery.clone(), b"overflow", |_| Ok(()))
            .unwrap();
        assert_eq!(history_bytes(&recovery).len(), MAX_HISTORY);
    }

    #[test]
    fn separate_sessions_serialize_shared_history_updates() {
        let root = TestDir::new("concurrent");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        std::fs::write(&primary, b"old").unwrap();
        let mut first = RecoverySession::default();
        let mut second = RecoverySession::default();
        first.begin_with(&primary, recovery.clone());
        second.begin_with(&primary, recovery.clone());

        let first_primary = primary.clone();
        let first_recovery = recovery.clone();
        let first = std::thread::spawn(move || {
            first
                .write_with(&first_primary, first_recovery, b"first", |_| Ok(()))
                .unwrap();
        });
        let second_primary = primary.clone();
        let second_recovery = recovery.clone();
        let second = std::thread::spawn(move || {
            second
                .write_with(&second_primary, second_recovery, b"second", |_| Ok(()))
                .unwrap();
        });
        first.join().unwrap();
        second.join().unwrap();

        let primary_bytes = std::fs::read(&primary).unwrap();
        assert!(primary_bytes == b"first" || primary_bytes == b"second");
        let history = history_bytes(&recovery);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], b"old");
        assert!(history[1] == b"first" || history[1] == b"second");
    }

    #[test]
    fn zero_length_sram_is_committed_without_fake_session_start() {
        let root = TestDir::new("zero");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        let mut session = RecoverySession::default();
        session.begin_with(&primary, recovery.clone());

        session
            .write_with(&primary, recovery.clone(), b"", |_| Ok(()))
            .unwrap();

        assert_eq!(std::fs::read(primary).unwrap(), b"");
        assert!(!recovery.exists());
    }

    #[test]
    fn oversized_primary_and_history_are_bounded() {
        let root = TestDir::new("oversized");
        let primary = root.0.join("primary.sav");
        let recovery = root.0.join("recovery");
        std::fs::File::create(&primary)
            .unwrap()
            .set_len(MAX_SRAM_BYTES + 1)
            .unwrap();
        let mut session = RecoverySession::default();
        session.begin_with(&primary, recovery.clone());

        assert!(
            session
                .write_with(&primary, recovery.clone(), b"small", |_| Ok(()))
                .is_err()
        );
        assert_eq!(
            std::fs::metadata(&primary).unwrap().len(),
            MAX_SRAM_BYTES + 1
        );

        let history_directory = recovery.join("history");
        std::fs::create_dir_all(&history_directory).unwrap();
        let previous = b"old";
        let hash = zeff_firmware::sha256_bytes(previous);
        let corrupt = history_directory.join(format!("{:016X}_{}.sav", 1, const_hex::encode(hash)));
        std::fs::File::create(&corrupt)
            .unwrap()
            .set_len(MAX_SRAM_BYTES + 1)
            .unwrap();
        let mut entry = RecoveryEntry {
            initial_primary: InitialPrimary::Captured(None),
            ..Default::default()
        };

        persist_recovery(&recovery, &mut entry, previous, &mut |_| Ok(())).unwrap();

        let history = read_history(&history_directory).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(std::fs::read(&history[1].path).unwrap(), previous);
    }
}
