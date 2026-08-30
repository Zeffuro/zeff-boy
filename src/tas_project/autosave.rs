#![cfg(not(target_arch = "wasm32"))]

use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

use super::{TasDigest, TasProject};

pub const DEFAULT_TAS_AUTOSAVE_GENERATIONS: usize = 3;
pub const MAX_TAS_AUTOSAVE_GENERATIONS: usize = 32;

const AUTOSAVE_PREFIX: &str = "ztas-autosave-";
const GENERATION_DIGITS: usize = 20;
const MAX_DIRECTORY_ENTRIES: usize = 1024;
const MAX_PUBLICATION_ATTEMPTS: usize = 16;
static AUTOSAVE_UPDATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TasAutosaveConfig {
    retain_generations: usize,
}

impl TasAutosaveConfig {
    pub fn new(retain_generations: usize) -> Result<Self> {
        if !(1..=MAX_TAS_AUTOSAVE_GENERATIONS).contains(&retain_generations) {
            bail!("TAS autosave retention must be in 1..={MAX_TAS_AUTOSAVE_GENERATIONS}");
        }
        Ok(Self { retain_generations })
    }

    pub fn retain_generations(self) -> usize {
        self.retain_generations
    }
}

impl Default for TasAutosaveConfig {
    fn default() -> Self {
        Self {
            retain_generations: DEFAULT_TAS_AUTOSAVE_GENERATIONS,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TasAutosaveStore {
    directory: crate::platform::StableDirectory,
    config: TasAutosaveConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TasAutosaveSave {
    pub generation: u64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TasAutosaveRecovery {
    pub generation: u64,
    pub path: PathBuf,
    pub project: TasProject,
}

impl TasAutosaveStore {
    pub fn new(directory: impl AsRef<Path>, config: TasAutosaveConfig) -> Result<Self> {
        Ok(Self {
            directory: crate::platform::StableDirectory::open_or_create(
                directory.as_ref(),
                "TAS autosave directory",
            )?,
            config,
        })
    }

    pub fn beside_manual_save(
        manual_project_path: &Path,
        config: TasAutosaveConfig,
    ) -> Result<Self> {
        if !TasProject::is_project_path(manual_project_path) {
            bail!("manual TAS project must use the .ztas extension");
        }
        let parent = manual_project_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let namespace = manual_save_namespace(manual_project_path)?;
        Self::new(parent.join("autosave").join(namespace), config)
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn save(&self, project: &TasProject) -> Result<TasAutosaveSave> {
        let bytes = project.encode().context("failed to encode TAS autosave")?;
        let project_key = project_key(&project.project_id);
        let _guard = AUTOSAVE_UPDATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            let generations = self.scan_generations(&project_key)?;
            let generation = next_generation(&generations, project.edit_generation)?;
            let path = autosave_path(self.directory.path(), &project_key, generation);

            self.revalidate_directory()?;
            match publish_snapshot(&path, &bytes, project) {
                Ok(()) => {
                    self.rotate(&project_key).with_context(|| {
                        format!(
                            "TAS autosave {} was published but rotation failed",
                            path.display()
                        )
                    })?;
                    return Ok(TasAutosaveSave { generation, path });
                }
                Err(error) if path.exists() => {
                    // A final sync failure can leave a complete target.
                    self.revalidate_directory()?;
                    if TasProject::load(&path).is_ok_and(|saved| saved == *project) {
                        self.revalidate_directory()?;
                        return Err(error).with_context(|| {
                            format!(
                                "TAS autosave {} was published but final durability failed",
                                path.display()
                            )
                        });
                    }
                }
                Err(error) => return Err(error),
            }
        }

        bail!(
            "could not allocate a TAS autosave generation after {MAX_PUBLICATION_ATTEMPTS} attempts"
        )
    }

    pub fn recover_newest(&self, project_id: &str) -> Result<Option<TasAutosaveRecovery>> {
        let project_key = project_key(project_id);
        let _guard = AUTOSAVE_UPDATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generations = self.scan_generations(&project_key)?;
        if generations.is_empty() {
            return Ok(None);
        }

        let mut newest_failure = None;
        for candidate in generations.iter().rev() {
            self.revalidate_directory()?;
            let loaded = TasProject::load(&candidate.path).and_then(|project| {
                if project.project_id != project_id {
                    bail!("autosave project ID does not match its filename key");
                }
                if project.edit_generation > candidate.generation {
                    bail!("autosave filename generation precedes its project edit generation");
                }
                Ok(project)
            });
            match loaded {
                Ok(project) => {
                    self.revalidate_directory()?;
                    return Ok(Some(TasAutosaveRecovery {
                        generation: candidate.generation,
                        path: candidate.path.clone(),
                        project,
                    }));
                }
                Err(error) if newest_failure.is_none() => newest_failure = Some(error),
                Err(_) => {}
            }
        }

        let newest = generations
            .last()
            .expect("non-empty autosave generations should have a newest entry");
        let newest_failure = newest_failure
            .expect("each rejected autosave generation should report a validation failure");
        Err(newest_failure).with_context(|| {
            format!(
                "no valid TAS autosave for project {project_id:?} among {} generations; newest was {}",
                generations.len(),
                newest.path.display()
            )
        })
    }

    fn rotate(&self, project_key: &str) -> Result<()> {
        let generations = self.scan_generations(project_key)?;
        let remove_count = generations
            .len()
            .saturating_sub(self.config.retain_generations);
        for candidate in generations.into_iter().take(remove_count) {
            self.revalidate_directory()?;
            if candidate.path.parent() != Some(self.directory.path()) {
                bail!("TAS autosave rotation candidate escaped its dedicated directory");
            }
            let metadata = match fs::symlink_metadata(&candidate.path) {
                Ok(metadata) if metadata.file_type().is_file() => metadata,
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if crate::platform::metadata_is_redirect(&metadata) {
                continue;
            }
            match std::fs::remove_file(&candidate.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to prune TAS autosave generation {}",
                            candidate.path.display()
                        )
                    });
                }
            }
            self.revalidate_directory()?;
        }
        Ok(())
    }

    fn scan_generations(&self, project_key: &str) -> Result<Vec<AutosaveGeneration>> {
        self.revalidate_directory()?;
        let generations = scan_generations(self.directory.path(), project_key)?;
        self.revalidate_directory()?;
        Ok(generations)
    }

    fn revalidate_directory(&self) -> Result<()> {
        self.directory.revalidate()
    }
}

#[derive(Debug)]
struct AutosaveGeneration {
    generation: u64,
    path: PathBuf,
}

fn project_key(project_id: &str) -> String {
    TasDigest::from_bytes(project_id.as_bytes()).to_hex()
}

fn manual_save_namespace(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve the TAS project path")?
            .join(path)
    };
    let identity = absolute.to_string_lossy().into_owned();
    #[cfg(windows)]
    let identity = identity.to_ascii_lowercase();
    Ok(TasDigest::from_bytes(identity.as_bytes()).to_hex())
}

fn autosave_path(directory: &Path, project_key: &str, generation: u64) -> PathBuf {
    directory.join(format!(
        "{AUTOSAVE_PREFIX}{project_key}-{generation:0GENERATION_DIGITS$}.ztas"
    ))
}

fn next_generation(generations: &[AutosaveGeneration], edit_generation: u64) -> Result<u64> {
    let Some(previous) = generations.last() else {
        return Ok(edit_generation);
    };
    if previous.generation < edit_generation {
        Ok(edit_generation)
    } else {
        previous
            .generation
            .checked_add(1)
            .context("TAS autosave generation overflow")
    }
}

fn scan_generations(directory: &Path, project_key: &str) -> Result<Vec<AutosaveGeneration>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to scan TAS autosave directory {}",
                    directory.display()
                )
            });
        }
    };
    let mut generations = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            bail!("TAS autosave directory exceeds the {MAX_DIRECTORY_ENTRIES}-entry scan limit");
        }
        let entry = entry.with_context(|| {
            format!(
                "failed to read TAS autosave directory {}",
                directory.display()
            )
        })?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(generation) = parse_generation_name(&name, project_key) else {
            continue;
        };
        generations.push(AutosaveGeneration {
            generation,
            path: entry.path(),
        });
    }
    generations.sort_by_key(|candidate| candidate.generation);
    Ok(generations)
}

fn parse_generation_name(name: &str, project_key: &str) -> Option<u64> {
    let generation = name
        .strip_prefix(AUTOSAVE_PREFIX)?
        .strip_prefix(project_key)?
        .strip_prefix('-')?
        .strip_suffix(".ztas")?;
    if generation.len() != GENERATION_DIGITS
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    generation.parse().ok()
}

fn publish_snapshot(path: &Path, bytes: &[u8], expected: &TasProject) -> Result<()> {
    crate::platform::write_new_file_atomically_validated(path, bytes, |temp_file| {
        validate_held_snapshot(temp_file, bytes.len(), expected)
    })
    .with_context(|| {
        format!(
            "failed to atomically publish TAS autosave {}",
            path.display()
        )
    })
}

fn validate_held_snapshot(
    temp_file: &mut File,
    expected_len: usize,
    expected: &TasProject,
) -> Result<()> {
    temp_file.rewind()?;
    let read_limit = u64::try_from(expected_len)
        .context("TAS autosave length does not fit u64")?
        .checked_add(1)
        .context("TAS autosave validation length overflow")?;
    let mut temp_bytes = Vec::with_capacity(expected_len);
    temp_file.take(read_limit).read_to_end(&mut temp_bytes)?;
    if temp_bytes.len() != expected_len {
        bail!("temporary TAS autosave length changed before validation");
    }
    let decoded = TasProject::decode(&temp_bytes)
        .context("temporary TAS autosave failed complete package validation")?;
    if decoded != *expected {
        bail!("temporary TAS autosave changed semantics before publication");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::tas_project::tests::project;

    fn generation_numbers(store: &TasAutosaveStore, project_id: &str) -> Vec<u64> {
        store
            .scan_generations(&project_key(project_id))
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.generation)
            .collect()
    }

    #[test]
    fn autosave_is_a_complete_cache_free_snapshot_and_preserves_manual_save() {
        let root = crate::test_support::test_directory("tas-autosave-complete").unwrap();
        let manual_path = root.path().join("movie.ztas");
        let mut project = project();
        project.project_id = "project:with..path-safe-key".to_owned();
        project.save_atomic(&manual_path).unwrap();
        let manual_bytes = std::fs::read(&manual_path).unwrap();

        let store =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())
                .unwrap();
        let saved = store.save(&project).unwrap();

        assert_eq!(std::fs::read(&manual_path).unwrap(), manual_bytes);
        assert_eq!(saved.path.parent(), Some(store.directory()));
        let saved_name = saved.path.file_name().unwrap().to_str().unwrap();
        assert!(!saved_name.contains(':'));
        assert!(!saved_name.contains(".."));
        assert_eq!(TasProject::load(&saved.path).unwrap(), project);

        let bytes = std::fs::read(&saved.path).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "manifest.json"));
        assert!(names.iter().any(|name| name == "integrity.json"));
        assert!(names.iter().any(|name| name == "start_state.bin"));
        assert!(names.iter().all(|name| {
            let name = name.to_ascii_lowercase();
            !name.contains("seek") && !name.contains("cache")
        }));
    }

    #[test]
    fn generations_are_monotonic_and_rotation_is_bounded() {
        let root = crate::test_support::test_directory("tas-autosave-rotation").unwrap();
        let store = TasAutosaveStore::new(root.path(), TasAutosaveConfig::new(3).unwrap()).unwrap();
        let mut project = project();

        let first = store.save(&project).unwrap();
        project.project_comment = "second autosave".to_owned();
        let second = store.save(&project).unwrap();
        project.edit_generation = 9;
        project.project_comment = "third autosave".to_owned();
        let third = store.save(&project).unwrap();
        project.project_comment = "fourth autosave".to_owned();
        let fourth = store.save(&project).unwrap();

        assert_eq!(
            [
                first.generation,
                second.generation,
                third.generation,
                fourth.generation,
            ],
            [3, 4, 9, 10]
        );
        assert_eq!(generation_numbers(&store, &project.project_id), [4, 9, 10]);
        assert!(!first.path.exists());
        for path in [&second.path, &third.path, &fourth.path] {
            TasProject::load(path).unwrap();
        }
        let recovered = store.recover_newest(&project.project_id).unwrap().unwrap();
        assert_eq!(recovered.generation, 10);
        assert_eq!(recovered.project, project);
    }

    #[test]
    fn manual_project_paths_have_independent_recovery_namespaces() {
        let root = crate::test_support::test_directory("tas-autosave-project-paths").unwrap();
        let first_path = root.path().join("first.ztas");
        let second_path = root.path().join("second.ztas");
        let mut first = project();
        let mut second = first.clone();
        first.project_comment = "first timeline".to_owned();
        second.project_comment = "second timeline".to_owned();
        first.save_atomic(&first_path).unwrap();
        second.save_atomic(&second_path).unwrap();

        let first_store =
            TasAutosaveStore::beside_manual_save(&first_path, TasAutosaveConfig::default())
                .unwrap();
        let second_store =
            TasAutosaveStore::beside_manual_save(&second_path, TasAutosaveConfig::default())
                .unwrap();
        assert_ne!(first_store.directory(), second_store.directory());

        first_store.save(&first).unwrap();
        second_store.save(&second).unwrap();
        assert_eq!(
            first_store
                .recover_newest(&first.project_id)
                .unwrap()
                .unwrap()
                .project,
            first
        );
        assert_eq!(
            second_store
                .recover_newest(&second.project_id)
                .unwrap()
                .unwrap()
                .project,
            second
        );
    }

    #[test]
    fn recovery_skips_corrupt_and_mismatched_newer_generations() {
        let root = crate::test_support::test_directory("tas-autosave-corruption").unwrap();
        let store = TasAutosaveStore::new(root.path(), TasAutosaveConfig::new(4).unwrap()).unwrap();
        let original = project();
        let first = store.save(&original).unwrap();
        let mut newer = original.clone();
        newer.project_comment = "newer".to_owned();
        let second = store.save(&newer).unwrap();
        std::fs::write(&second.path, b"corrupt autosave").unwrap();

        let mut wrong_project = newer.clone();
        wrong_project.project_id = "other-project".to_owned();
        let mismatched_path = autosave_path(
            root.path(),
            &project_key(&original.project_id),
            second.generation + 1,
        );
        std::fs::write(&mismatched_path, wrong_project.encode().unwrap()).unwrap();

        let recovered = store.recover_newest(&original.project_id).unwrap().unwrap();
        assert_eq!(recovered.generation, first.generation);
        assert_eq!(recovered.project, original);

        std::fs::write(&first.path, b"also corrupt").unwrap();
        let error = store
            .recover_newest(&newer.project_id)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no valid TAS autosave"));
    }

    #[test]
    fn failed_save_does_not_rotate_or_damage_existing_recovery() {
        let root = crate::test_support::test_directory("tas-autosave-failure").unwrap();
        let store = TasAutosaveStore::new(root.path(), TasAutosaveConfig::new(1).unwrap()).unwrap();
        let project = project();
        let saved = store.save(&project).unwrap();
        let saved_bytes = std::fs::read(&saved.path).unwrap();

        let mut invalid = project.clone();
        invalid.identity.start_state_sha256.0[0] ^= 1;
        assert!(store.save(&invalid).is_err());

        assert_eq!(generation_numbers(&store, &project.project_id), [3]);
        assert_eq!(std::fs::read(&saved.path).unwrap(), saved_bytes);
        assert_eq!(
            store
                .recover_newest(&project.project_id)
                .unwrap()
                .unwrap()
                .project,
            project
        );
    }

    #[test]
    fn no_replace_collision_and_generation_overflow_fail_safely() {
        let root = crate::test_support::test_directory("tas-autosave-collision").unwrap();
        let store = TasAutosaveStore::new(root.path(), TasAutosaveConfig::default()).unwrap();
        let project = project();
        let key = project_key(&project.project_id);
        let collision = autosave_path(root.path(), &key, project.edit_generation);
        std::fs::write(&collision, b"preexisting bytes").unwrap();

        let saved = store.save(&project).unwrap();
        assert_eq!(saved.generation, project.edit_generation + 1);
        assert_eq!(std::fs::read(&collision).unwrap(), b"preexisting bytes");

        let exhausted = autosave_path(root.path(), &key, u64::MAX);
        std::fs::write(&exhausted, b"last generation").unwrap();
        assert!(store.save(&project).is_err());
        assert_eq!(std::fs::read(&exhausted).unwrap(), b"last generation");

        assert!(TasAutosaveConfig::new(0).is_err());
        assert!(TasAutosaveConfig::new(MAX_TAS_AUTOSAVE_GENERATIONS + 1).is_err());
    }

    #[test]
    fn symlink_directory_is_rejected_without_touching_its_target() {
        let root = crate::test_support::test_directory("tas-autosave-symlink").unwrap();
        let target = root.path().join("target");
        let link = root.path().join("autosave-link");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"), b"unchanged").unwrap();
        if !create_directory_symlink(&target, &link) {
            return;
        }

        assert!(TasAutosaveStore::new(&link, TasAutosaveConfig::default()).is_err());
        assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    }

    #[test]
    fn replacing_directory_after_open_fails_closed() {
        let root = crate::test_support::test_directory("tas-autosave-substitution").unwrap();
        let directory = root.path().join("autosave");
        let displaced = root.path().join("displaced");
        let store = TasAutosaveStore::new(&directory, TasAutosaveConfig::default()).unwrap();
        let project = project();
        let saved = store.save(&project).unwrap();
        let saved_name = saved.path.file_name().unwrap().to_owned();

        fs::rename(&directory, &displaced).unwrap();
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("sentinel"), b"replacement").unwrap();

        let save_error = store.save(&project).unwrap_err().to_string();
        assert!(save_error.contains("replaced after it was opened"));
        let recovery_error = store
            .recover_newest(&project.project_id)
            .unwrap_err()
            .to_string();
        assert!(recovery_error.contains("replaced after it was opened"));
        assert_eq!(
            fs::read(directory.join("sentinel")).unwrap(),
            b"replacement"
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        assert_eq!(
            TasProject::load(&displaced.join(saved_name)).unwrap(),
            project
        );
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> bool {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => true,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
                ) =>
            {
                false
            }
            Err(error) => panic!("failed to create test directory symlink: {error}"),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn create_directory_symlink(_target: &Path, _link: &Path) -> bool {
        false
    }
}
