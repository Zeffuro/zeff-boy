use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::direct_fds::{
    MAX_FDS_IMAGE_BYTES, direct_fds_tas_sync_config_sha256, encode_zip_fds_asset,
    fds_project_disk_bytes, fds_tas_identity, fds_tas_side_count_supported,
    validate_fds_tas_branch_scope, validate_fds_tas_private_runtime,
    validate_fds_tas_project_identity, zip_fds_tas_sync_config_sha256,
};
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, MAX_NES_ZIP_BYTES, TasDigest,
    TasEditorExecutionEngine, TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch,
    TasProject, TasZrplImportWitness, has_extension, publish_new_project,
};

#[derive(Clone, Debug)]
pub(crate) struct DirectFdsTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    firmware_search_dirs: Vec<PathBuf>,
    owned_disk: Option<OwnedFdsDisk>,
    #[cfg(test)]
    fds_bios_override: Option<&'static [u8]>,
}

#[derive(Clone, Debug)]
struct OwnedFdsDisk {
    bytes: Vec<u8>,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    zip_member_name: Option<String>,
}

impl DirectFdsTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
            owned_disk: None,
            #[cfg(test)]
            fds_bios_override: None,
        }
    }

    pub(crate) fn new_zip(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            source_path,
            rom_path,
            firmware_search_dirs,
            owned_disk: None,
            #[cfg(test)]
            fds_bios_override: None,
        }
    }

    pub(crate) fn new_for_project(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        project: &TasProject,
    ) -> Result<Self> {
        validate_fds_tas_project_identity(project)?;
        Ok(Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
            owned_disk: Some(OwnedFdsDisk {
                bytes: fds_project_disk_bytes(project)?.to_vec(),
                source_media_sha256: project.identity().source_media_sha256,
                sync_config_sha256: project.identity().sync_config_sha256,
                zip_member_name: None,
            }),
            #[cfg(test)]
            fds_bios_override: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_with_bios_override(
        source_path: PathBuf,
        fds_bios_override: &'static [u8],
    ) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs: Vec::new(),
            owned_disk: None,
            fds_bios_override: Some(fds_bios_override),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_zip_with_bios_override(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        fds_bios_override: &'static [u8],
    ) -> Self {
        Self {
            source_path,
            rom_path,
            firmware_search_dirs: Vec::new(),
            owned_disk: None,
            fds_bios_override: Some(fds_bios_override),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_project_bios_override(mut self, bios: &'static [u8]) -> Self {
        self.fds_bios_override = Some(bios);
        self
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_fds_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let owned = self.read_creation_disk()?;
        let backend = self.load_backend(&owned)?;
        let snapshot = backend
            .media_slot_snapshot()
            .context("FDS creation backend omitted its drive state")?;
        ensure!(
            snapshot.inserted()
                && snapshot.state.side == Some(0)
                && !snapshot.state.write_protected
                && snapshot.state.mutation_counter == 0,
            "FDS creation requires the initial inserted writable side-zero drive state"
        );
        let start_state = backend.encode_state_bytes()?;
        let identity = fds_tas_identity(
            &backend,
            owned.source_media_sha256,
            owned.sync_config_sha256,
            &start_state,
        )?;
        let asset = match &owned.zip_member_name {
            Some(member_name) => encode_zip_fds_asset(member_name, &owned.bytes)?,
            None => owned.bytes.clone(),
        };
        let digest = TasDigest::from_bytes(&asset);
        TasProject::new(
            format!("fds-{}", identity.source_media_sha256.to_hex()),
            identity,
            start_state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: Vec::new(),
                events: Vec::new(),
            },
            BTreeMap::from([(digest, asset)]),
        )
    }

    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        let project = self.create_project()?;
        publish_new_project(path, &project)?;
        Ok(project)
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path) && path.exists(),
            "TAS project destination does not exist or has the wrong extension"
        );
        let project = self.create_project()?;
        project.save_atomic(path)?;
        Ok(project)
    }

    pub(crate) fn replay_import_witness(
        &self,
        start_state: &[u8],
    ) -> Result<(TasZrplImportWitness, BTreeMap<TasDigest, Vec<u8>>)> {
        let owned = self.read_creation_disk()?;
        let mut loader = self.clone();
        loader.owned_disk = Some(owned.clone());
        let identity = loader.load_session(start_state)?.identity().clone();
        let asset = match &owned.zip_member_name {
            Some(member_name) => encode_zip_fds_asset(member_name, &owned.bytes)?,
            None => owned.bytes,
        };
        Ok((
            TasZrplImportWitness {
                project_id: format!("fds-{}", identity.source_media_sha256.to_hex()),
                identity,
            },
            BTreeMap::from([(TasDigest::from_bytes(&asset), asset)]),
        ))
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        super::validate_current_nes_start_state(start_state)?;
        let owned = self
            .owned_disk
            .as_ref()
            .context("opening an FDS TAS session requires project-owned disk media")?;
        let mut backend = self.load_backend(owned)?;
        backend
            .load_state_from_bytes(start_state.to_vec())
            .context("failed to restore project-owned FDS native state")?;
        validate_fds_tas_private_runtime(&backend, false)?;
        let snapshot = backend
            .media_slot_snapshot()
            .context("FDS TAS start state omitted its drive state")?;
        ensure!(
            snapshot.inserted()
                && snapshot.state.side == Some(0)
                && !snapshot.state.write_protected
                && snapshot.state.mutation_counter == 0,
            "FDS TAS start state must preserve the initial inserted writable side-zero drive state"
        );
        let identity = fds_tas_identity(
            &backend,
            owned.source_media_sha256,
            owned.sync_config_sha256,
            start_state,
        )?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        validate_fds_tas_project_identity(project)?;
        for branch in project.branches() {
            validate_fds_tas_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, validate_fds_tas_branch_scope)
    }

    #[cfg(test)]
    pub(crate) fn load_fresh_backend(&self) -> Result<EmuBackend> {
        let owned = self
            .owned_disk
            .as_ref()
            .context("opening an FDS TAS backend requires project-owned disk media")?;
        self.load_backend(owned)
    }

    fn read_creation_disk(&self) -> Result<OwnedFdsDisk> {
        if has_extension(&self.source_path, "fds") {
            let bytes = read_fds_bounded(&self.source_path)?;
            let side_count = validate_disk_bytes(&bytes, false)?;
            return Ok(OwnedFdsDisk {
                source_media_sha256: TasDigest::from_bytes(&bytes),
                sync_config_sha256: direct_fds_tas_sync_config_sha256(side_count)?,
                zip_member_name: None,
                bytes,
            });
        }
        if has_extension(&self.source_path, "zip") {
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "fds",
                MAX_NES_ZIP_BYTES,
                MAX_FDS_IMAGE_BYTES,
            )?;
            let side_count = validate_disk_bytes(&selected.bytes, true)?;
            return Ok(OwnedFdsDisk {
                bytes: selected.bytes,
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: zip_fds_tas_sync_config_sha256(
                    &selected.member_name,
                    side_count,
                )?,
                zip_member_name: Some(selected.member_name),
            });
        }
        bail!("FDS TAS execution requires a direct .fds file or selected ZIP member")
    }

    fn load_backend(&self, owned: &OwnedFdsDisk) -> Result<EmuBackend> {
        validate_disk_bytes(&owned.bytes, false)?;
        let virtual_path = self.source_path.with_file_name("project-owned.fds");
        let config = BackendLoadConfig {
            firmware_search_dirs: self.firmware_search_dirs.clone(),
            sample_rate: Some(48_000),
            apply_mods: false,
            initial_input: None,
            nes_load_battery_sram: false,
            #[cfg(test)]
            fds_bios_override: self.fds_bios_override,
            ..BackendLoadConfig::default()
        };
        let mut backend = super::load_backend_from_rom_source(
            ActiveSystem::Nes,
            &virtual_path,
            &virtual_path,
            Some(owned.bytes.clone()),
            config,
        )?
        .backend;
        let EmuBackend::Nes(nes) = &mut backend else {
            bail!("FDS loader did not produce a NES backend");
        };
        nes.set_fds_tas_media_identity(owned.source_media_sha256.0, owned.sync_config_sha256.0);
        nes.set_host_persistence_enabled(false);
        validate_fds_tas_private_runtime(&backend, false)?;
        Ok(backend)
    }
}

fn read_fds_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open FDS TAS source {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_FDS_IMAGE_BYTES,
        "FDS TAS source exceeds the bounded disk-image limit"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_FDS_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FDS_IMAGE_BYTES,
        "FDS TAS source exceeds the bounded disk-image limit"
    );
    Ok(bytes)
}

fn validate_disk_bytes(bytes: &[u8], require_headerless: bool) -> Result<usize> {
    let image = zeff_nes_core::hardware::cartridge::mappers::FdsImage::parse(bytes)?;
    ensure!(
        fds_tas_side_count_supported(image.side_count()),
        "FDS TAS execution supports one through 255 disk sides"
    );
    ensure!(
        !require_headerless || !image.has_header(),
        "ZIP FDS TAS execution requires a headerless selected member"
    );
    Ok(image.side_count())
}

impl TasEditorExecutionProvider for DirectFdsTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectFdsTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod tests;
