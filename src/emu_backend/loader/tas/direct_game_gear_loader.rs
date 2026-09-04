use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_sega8_core::hardware::cartridge::{
    GameGearCartridgeIdentity, GameGearStandardMapperRam, GameGearStandardMapperRamIdentity,
    game_gear_standard_mapper_ram_for_identity,
    game_gear_standard_mapper_ram_identity_from_catalog_entry,
};

use super::media::{read_bounded_direct_rom, reject_embedded_zip_sram};
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_game_gear::{
        DirectGameGearTasBoardChoice, direct_game_gear_tas_board_choice,
        direct_game_gear_tas_identity, zip_game_gear_tas_board_choice,
    },
    has_extension,
};

pub(crate) const MAX_DIRECT_GAME_GEAR_ROM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_GAME_GEAR_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
enum BoardCatalog {
    Production,
    ConfirmedNoCartridgeSaveMemory,
    #[cfg(test)]
    Injected(GameGearStandardMapperRamIdentity),
}

#[cfg(test)]
static TEST_BOARD_CATALOG: Mutex<Vec<(GameGearCartridgeIdentity, GameGearStandardMapperRam)>> =
    Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) struct TestGameGearBoardCatalogGuard {
    identity: GameGearCartridgeIdentity,
}

#[cfg(test)]
impl Drop for TestGameGearBoardCatalogGuard {
    fn drop(&mut self) {
        let mut catalog = TEST_BOARD_CATALOG
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = catalog
            .iter()
            .position(|(identity, _)| *identity == self.identity)
        {
            catalog.swap_remove(index);
        }
    }
}

#[cfg(test)]
pub(crate) fn register_test_game_gear_board_catalog_entry(
    identity: GameGearCartridgeIdentity,
    ram: GameGearStandardMapperRam,
) -> TestGameGearBoardCatalogGuard {
    let mut catalog = TEST_BOARD_CATALOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(catalog.iter().all(|(existing, _)| *existing != identity));
    catalog.push((identity, ram));
    TestGameGearBoardCatalogGuard { identity }
}

fn exact_game_gear_standard_mapper_ram_for_identity(
    identity: GameGearCartridgeIdentity,
) -> Option<GameGearStandardMapperRamIdentity> {
    let production = game_gear_standard_mapper_ram_for_identity(identity);
    #[cfg(not(test))]
    return production;
    #[cfg(test)]
    production.or_else(|| {
        TEST_BOARD_CATALOG
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(entry, _)| *entry == identity)
            .map(|(entry, ram)| {
                game_gear_standard_mapper_ram_identity_from_catalog_entry(*entry, *ram)
            })
    })
}

#[derive(Clone, Debug)]
pub(crate) struct DirectGameGearTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    board_catalog: BoardCatalog,
}

pub(crate) struct GameGearTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    board_choice: DirectGameGearTasBoardChoice,
}

impl DirectGameGearTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf) -> Self {
        Self {
            source_path,
            rom_path: None,
            board_catalog: BoardCatalog::Production,
        }
    }

    pub(crate) fn new_with_confirmed_no_cartridge_save_memory(source_path: PathBuf) -> Self {
        Self {
            source_path,
            rom_path: None,
            board_catalog: BoardCatalog::ConfirmedNoCartridgeSaveMemory,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_catalog_entry(
        source_path: PathBuf,
        identity: GameGearCartridgeIdentity,
        ram: GameGearStandardMapperRam,
    ) -> Self {
        Self {
            source_path,
            rom_path: None,
            board_catalog: BoardCatalog::Injected(
                game_gear_standard_mapper_ram_identity_from_catalog_entry(identity, ram),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_zip_with_catalog_entry(
        source_path: PathBuf,
        rom_path: PathBuf,
        identity: GameGearCartridgeIdentity,
        ram: GameGearStandardMapperRam,
    ) -> Self {
        Self {
            source_path,
            rom_path: Some(rom_path),
            board_catalog: BoardCatalog::Injected(
                game_gear_standard_mapper_ram_identity_from_catalog_entry(identity, ram),
            ),
        }
    }

    pub(crate) fn new_zip(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        confirmed_no_save: bool,
    ) -> Self {
        Self {
            source_path,
            rom_path,
            board_catalog: if confirmed_no_save {
                BoardCatalog::ConfirmedNoCartridgeSaveMemory
            } else {
                BoardCatalog::Production
            },
        }
    }

    pub(crate) fn new_zip_for_project(source_path: PathBuf, project: &TasProject) -> Result<Self> {
        super::direct_game_gear::validate_direct_game_gear_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "gg",
            MAX_GAME_GEAR_ZIP_BYTES,
            MAX_DIRECT_GAME_GEAR_ROM_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "Game Gear ZIP archive does not match the TAS project"
        );
        let mut matches = Vec::new();
        for entry in inspection.entries {
            if let Ok(board) =
                zip_game_gear_tas_board_choice(project.identity(), &entry.member_name)
            {
                matches.push((entry, board));
            }
        }
        ensure!(
            matches.len() == 1,
            "Game Gear ZIP member does not match the TAS project"
        );
        let (entry, board) = matches.remove(0);
        Ok(Self::new_zip(
            source_path,
            Some(entry.rom_path),
            board == DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
        ))
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        super::direct_game_gear::validate_direct_game_gear_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn requires_confirmed_no_cartridge_save_memory(&self) -> Result<bool> {
        ensure!(
            matches!(self.board_catalog, BoardCatalog::Production),
            "Game Gear board confirmation is only available for an unclassified cartridge"
        );
        let source_bytes = self.source_bytes()?;
        let cartridge_identity = GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&source_bytes),
            source_len: source_bytes.len(),
        };
        match exact_game_gear_standard_mapper_ram_for_identity(cartridge_identity) {
            Some(identity)
                if matches!(
                    identity.ram(),
                    GameGearStandardMapperRam::Absent
                        | GameGearStandardMapperRam::BatteryBacked8KiB
                ) =>
            {
                Ok(false)
            }
            Some(_) => anyhow::bail!("Game Gear cartridge has unsupported mapper RAM"),
            None => {
                Self::new_zip(self.source_path.clone(), self.rom_path.clone(), true)
                    .load_fresh_backend()?;
                Ok(true)
            }
        }
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("game-gear-{}", identity.source_media_sha256.to_hex()),
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
            BTreeMap::new(),
        )
    }

    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        let project = self.create_project()?;
        super::publish_new_project(path, &project)?;
        Ok(project)
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        ensure!(path.exists(), "TAS project destination does not exist");
        let project = self.create_project()?;
        project.save_atomic(path).with_context(|| {
            format!(
                "failed to atomically replace TAS project {}",
                path.display()
            )
        })?;
        Ok(project)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        let (mut backend, media) = self.load_fresh_backend()?;
        if backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None {
            ensure!(
                backend.encode_state_bytes()?.as_slice() == start_state,
                "Game Gear TAS starting state does not match the fresh direct-ROM baseline"
            );
        }
        let projection = super::direct_game_gear::validate_direct_game_gear_tas_private_state(
            &mut backend,
            start_state,
        )?;
        ensure!(
            projection.frame_count == 0 && projection.framebuffer.as_ref() == backend.framebuffer(),
            "Game Gear TAS starting state does not restore the fresh baseline frame"
        );
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id())?;
        }
        let session = self
            .with_board_choice(self.project_board_choice(project)?)
            .load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(
            project,
            session,
            super::direct_game_gear::validate_direct_game_gear_tas_branch_scope,
        )
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, GameGearTasMediaIdentity)> {
        self.load_backend(true)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, GameGearTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_backend(
        &self,
        load_battery_sram: bool,
    ) -> Result<(EmuBackend, GameGearTasMediaIdentity)> {
        let selected = if has_extension(&self.source_path, "zip") {
            Some(crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "gg",
                MAX_GAME_GEAR_ZIP_BYTES,
                MAX_DIRECT_GAME_GEAR_ROM_BYTES,
            )?)
        } else {
            None
        };
        let source_bytes = selected
            .as_ref()
            .map(|entry| entry.bytes.clone())
            .map_or_else(|| read_direct_game_gear_rom(&self.source_path), Ok)?;
        let cartridge_identity = GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&source_bytes),
            source_len: source_bytes.len(),
        };
        let board_identity = match self.board_catalog {
            BoardCatalog::Production => {
                exact_game_gear_standard_mapper_ram_for_identity(cartridge_identity)
            }
            BoardCatalog::ConfirmedNoCartridgeSaveMemory => {
                Some(game_gear_standard_mapper_ram_identity_from_catalog_entry(
                    cartridge_identity,
                    GameGearStandardMapperRam::Absent,
                ))
            }
            #[cfg(test)]
            BoardCatalog::Injected(identity) => Some(identity),
        };
        let board_choice = match (
            self.board_catalog,
            board_identity.map(|identity| identity.ram()),
        ) {
            (BoardCatalog::ConfirmedNoCartridgeSaveMemory, _) => {
                DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory
            }
            (_, Some(GameGearStandardMapperRam::Absent)) => {
                DirectGameGearTasBoardChoice::CataloguedAbsent
            }
            (_, Some(GameGearStandardMapperRam::BatteryBacked8KiB)) => {
                DirectGameGearTasBoardChoice::CataloguedBattery8KiB
            }
            _ => {
                anyhow::bail!("Game Gear cartridge lacks an exact supported board catalogue entry")
            }
        };
        if board_choice == DirectGameGearTasBoardChoice::CataloguedBattery8KiB && selected.is_some()
        {
            reject_embedded_zip_sram(
                &self.source_path,
                MAX_GAME_GEAR_ZIP_BYTES,
                MAX_DIRECT_GAME_GEAR_ROM_BYTES,
                "Game Gear TAS ZIP must not embed a battery save",
            )?;
        }
        let config = BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            game_gear_standard_mapper_ram_identity: board_identity,
            sega8_load_battery_sram: load_battery_sram,
            sega8_video_standard: Some(zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc),
            sega8_console_region: Some(zeff_sega8_core::hardware::region::Sega8Region::Export),
            sega8_use_external_boot_rom: false,
            ..BackendLoadConfig::default()
        };
        let mut backend = if let Some(selected) = selected.as_ref() {
            super::load_backend_from_rom_source(
                ActiveSystem::GameGear,
                &self.source_path,
                &selected.rom_path,
                Some(source_bytes.clone()),
                config,
            )?
            .backend
        } else {
            crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::GameGear,
                &self.source_path,
                source_bytes.clone(),
                config,
            )?
            .backend
        };
        let media = if let Some(selected) = selected {
            GameGearTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: super::direct_game_gear::zip_game_gear_tas_sync_config_sha256(
                    board_choice,
                    &selected.member_name,
                ),
                board_choice,
            }
        } else {
            GameGearTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                sync_config_sha256:
                    super::direct_game_gear::direct_game_gear_tas_sync_config_sha256_for_board(
                        board_choice,
                    ),
                board_choice,
            }
        };
        let EmuBackend::Sega8(sega8) = &mut backend else {
            unreachable!("direct Game Gear loader must produce a Sega 8-bit backend");
        };
        sega8.set_game_gear_tas_sync_config_sha256(media.sync_config_sha256.0);
        super::direct_game_gear::validate_direct_game_gear_tas_private_runtime(&backend, false)?;
        Ok((backend, media))
    }

    fn with_board_choice(&self, board_choice: DirectGameGearTasBoardChoice) -> Self {
        if has_extension(&self.source_path, "zip") {
            #[cfg(test)]
            if let BoardCatalog::Injected(identity) = self.board_catalog {
                return Self {
                    source_path: self.source_path.clone(),
                    rom_path: self.rom_path.clone(),
                    board_catalog: BoardCatalog::Injected(identity),
                };
            }
            return Self::new_zip(
                self.source_path.clone(),
                self.rom_path.clone(),
                board_choice == DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
            );
        }
        match board_choice {
            DirectGameGearTasBoardChoice::CataloguedAbsent => match self.board_catalog {
                #[cfg(test)]
                BoardCatalog::Injected(identity) => Self {
                    source_path: self.source_path.clone(),
                    rom_path: None,
                    board_catalog: BoardCatalog::Injected(identity),
                },
                _ => Self::new(self.source_path.clone()),
            },
            DirectGameGearTasBoardChoice::CataloguedBattery8KiB => match self.board_catalog {
                #[cfg(test)]
                BoardCatalog::Injected(identity) => Self {
                    source_path: self.source_path.clone(),
                    rom_path: None,
                    board_catalog: BoardCatalog::Injected(identity),
                },
                _ => Self::new(self.source_path.clone()),
            },
            DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory => {
                Self::new_with_confirmed_no_cartridge_save_memory(self.source_path.clone())
            }
        }
    }

    fn source_bytes(&self) -> Result<Vec<u8>> {
        if has_extension(&self.source_path, "zip") {
            Ok(crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "gg",
                MAX_GAME_GEAR_ZIP_BYTES,
                MAX_DIRECT_GAME_GEAR_ROM_BYTES,
            )?
            .bytes)
        } else {
            read_direct_game_gear_rom(&self.source_path)
        }
    }

    fn project_board_choice(&self, project: &TasProject) -> Result<DirectGameGearTasBoardChoice> {
        if has_extension(&self.source_path, "zip") {
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "gg",
                MAX_GAME_GEAR_ZIP_BYTES,
                MAX_DIRECT_GAME_GEAR_ROM_BYTES,
            )?;
            zip_game_gear_tas_board_choice(project.identity(), &selected.member_name)
        } else {
            direct_game_gear_tas_board_choice(project.identity())
        }
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: GameGearTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if !has_extension(&self.source_path, "zip") {
            let bytes = read_direct_game_gear_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&bytes),
                "Game Gear source changed while constructing TAS identity"
            );
            return direct_game_gear_tas_identity(backend, &bytes, start_state, media.board_choice);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "gg",
            MAX_GAME_GEAR_ZIP_BYTES,
            MAX_DIRECT_GAME_GEAR_ROM_BYTES,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == super::direct_game_gear::zip_game_gear_tas_sync_config_sha256(
                        media.board_choice,
                        &selected.member_name
                    )
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "Game Gear ZIP changed while constructing TAS identity"
        );
        super::direct_game_gear::zip_game_gear_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
            media.board_choice,
        )
    }
}

impl TasEditorExecutionProvider for DirectGameGearTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectGameGearTasExecutionLoader::load_editor_engine(self, project)
    }
}

fn read_direct_game_gear_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (1..=MAX_DIRECT_GAME_GEAR_ROM_BYTES).contains(&metadata.len()),
        "direct Game Gear TAS media has an unsupported size"
    );
    let expected_len = usize::try_from(metadata.len()).context("Game Gear media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct Game Gear TAS media changed while it was read",
    )
}

#[cfg(test)]
mod tests;
