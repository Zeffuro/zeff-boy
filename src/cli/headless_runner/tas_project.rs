use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::tas_project::verification::TasExecutionSession;
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExecutionWitness, TasExternalIdentity,
    TasFirmwareIdentity, TasProject, TasProjectIdentity,
};

use super::HeadlessOptions;

const MAX_NES_CARTRIDGE_BYTES: u64 = 64 * 1024 * 1024;
const NES_GAMEPAD_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0nes-standard-controller\0";
const NES_CARTRIDGE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0external-state=absent\0";

pub(super) fn run_tas_project_headless(
    rom_source_path: &Path,
    firmware_search_dirs: Vec<PathBuf>,
    opts: &HeadlessOptions,
) -> Result<()> {
    let project_path = opts
        .tas_project_path
        .as_deref()
        .context("missing --tas-verify project path")?;
    let mut project = TasProject::load(project_path)
        .with_context(|| format!("failed to load TAS project {}", project_path.display()))?;
    let branch_id = opts
        .tas_branch_id
        .as_deref()
        .unwrap_or(project.active_branch_id.as_str())
        .to_owned();
    validate_nes_cartridge_project_scope(&project, &branch_id)?;

    let plan = TasExecutionPlan {
        source_path: rom_source_path.to_path_buf(),
        firmware_search_dirs,
    };
    let start_state = project.start_state.clone();
    let witness_session = plan.load_session(&start_state)?;
    let witness = TasExecutionWitness {
        identity: witness_session.identity().clone(),
    };
    let verification = project
        .verify_branch_with_factory(&branch_id, &witness, || plan.load_session(&start_state))?;

    project.save_atomic(project_path).with_context(|| {
        format!(
            "failed to save verified TAS project {}",
            project_path.display()
        )
    })?;
    println!(
        "[tas] verify project={} branch={} frames={} checkpoints={} final_state_sha256={} status=verified",
        project_path.display(),
        branch_id,
        project
            .branch(&branch_id)
            .expect("verified branch still exists")
            .frame_count,
        verification.checkpoints.len(),
        verification
            .final_state_sha256
            .map_or_else(|| "none".to_owned(), |digest| digest.to_hex()),
    );
    println!("[tas] project_saved={}", project_path.display());

    if let Some(export_path) = opts.tas_export_path.as_deref() {
        project.export_verified_zrpl_with_factory(&branch_id, export_path, &witness, || {
            plan.load_session(&start_state)
        })?;
        println!(
            "[tas] export status=exported output={}",
            export_path.display()
        );
    }
    Ok(())
}

#[derive(Clone)]
struct TasExecutionPlan {
    source_path: PathBuf,
    firmware_search_dirs: Vec<PathBuf>,
}

impl TasExecutionPlan {
    fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        validate_current_nes_start_state(start_state)?;
        ensure!(
            self.source_path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nes")),
            "TAS verification currently supports direct .nes cartridge files only"
        );
        let (rom_path, preloaded_data, system) =
            crate::app::detect_and_extract_rom(&self.source_path)?;
        ensure!(
            system == ActiveSystem::Nes && preloaded_data.is_none() && rom_path == self.source_path,
            "TAS verification currently supports cartridge NES media only"
        );
        let source_bytes = read_nes_cartridge_bounded(&self.source_path)?;
        let mut loaded = load_backend_from_rom_source(
            system,
            &self.source_path,
            &rom_path,
            Some(source_bytes.clone()),
            BackendLoadConfig {
                firmware_search_dirs: self.firmware_search_dirs.clone(),
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?;
        loaded
            .backend
            .load_state_from_bytes(start_state.to_vec())
            .context("failed to restore TAS starting state for device-profile validation")?;
        ensure!(
            loaded.backend.nes_has_standard_controller_topology() == Some(true),
            "TAS starting state does not restore the standard NES controller topology"
        );
        let identity = nes_cartridge_identity(&loaded.backend, &source_bytes, start_state)?;
        Ok(TasExecutionSession::new(loaded.backend, identity))
    }
}

fn nes_cartridge_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    ensure!(
        backend.system() == ActiveSystem::Nes,
        "TAS execution profile requires a NES backend"
    );
    let metadata = backend.replay_metadata();
    let system = metadata
        .system
        .context("NES backend omitted its system identity")?;
    let core_family = metadata
        .core_family
        .context("NES backend omitted its core-family identity")?;
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("NES backend omitted its effective media identity")?,
    );
    let source_media_sha256 = TasDigest::from_bytes(source_bytes);
    ensure!(
        source_media_sha256 == effective_media_sha256,
        "cartridge NES loader changed media bytes without a declared patch chain"
    );
    ensure!(
        metadata.cheat_sha256.is_none(),
        "cartridge NES execution unexpectedly enabled cheats"
    );
    ensure!(
        metadata.firmware.is_empty(),
        "cartridge NES execution unexpectedly selected firmware"
    );
    Ok(TasProjectIdentity {
        system,
        core_family,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: metadata
            .firmware
            .iter()
            .map(tas_firmware_identity)
            .collect(),
        devices: ["p1", "p2"]
            .into_iter()
            .map(|port| TasDeviceIdentity {
                port: port.to_owned(),
                device: "nes-standard-controller".to_owned(),
                configuration_sha256: TasDigest::from_bytes(NES_GAMEPAD_CONFIGURATION),
            })
            .collect(),
        sync_config_sha256: TasDigest::from_bytes(NES_CARTRIDGE_SYNC_CONFIGURATION),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

fn read_nes_cartridge_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open TAS source media {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_NES_CARTRIDGE_BYTES,
        "TAS source media exceeds the {MAX_NES_CARTRIDGE_BYTES}-byte cartridge limit"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_NES_CARTRIDGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read TAS source media {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_NES_CARTRIDGE_BYTES,
        "TAS source media exceeds the {MAX_NES_CARTRIDGE_BYTES}-byte cartridge limit"
    );
    Ok(bytes)
}

fn validate_current_nes_start_state(start_state: &[u8]) -> Result<()> {
    ensure!(
        start_state.len() >= 12
            && start_state[..8] == zeff_nes_core::save_state::NES_SAVE_STATE_MAGIC,
        "TAS starting state is not a native NES save state"
    );
    let version = u32::from_le_bytes(start_state[8..12].try_into().expect("length checked above"));
    ensure!(
        version == zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION,
        "TAS verification requires native NES state format {}, got {version}",
        zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION
    );
    let mut projected = start_state.to_vec();
    zeff_nes_core::save_state::project_replay_state_bytes(&mut projected)
        .context("TAS starting state failed canonical NES v11 validation")
}

fn tas_firmware_identity(firmware: &ReplayFirmwareManifest) -> TasFirmwareIdentity {
    match firmware {
        ReplayFirmwareManifest::External {
            firmware_id,
            variant,
            sha256,
        } => TasFirmwareIdentity::External {
            firmware_id: firmware_id.clone(),
            variant: variant.clone(),
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version,
        } => TasFirmwareIdentity::Hle {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
        },
        ReplayFirmwareManifest::BuiltinOpenSource {
            firmware_id,
            implementation,
            compatibility_version,
            sha256,
        } => TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Skipped {
            firmware_id,
            compatibility_version,
        } => TasFirmwareIdentity::Skipped {
            firmware_id: firmware_id.clone(),
            compatibility_version: *compatibility_version,
        },
    }
}

fn validate_nes_cartridge_project_scope(project: &TasProject, branch_id: &str) -> Result<()> {
    let branch = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        project.replay_start == Default::default(),
        "cartridge NES TAS verification does not support replay start metadata"
    );
    ensure!(
        branch.events.is_empty(),
        "cartridge NES TAS verification does not support synchronized media or link events"
    );
    for span in &branch.input_spans {
        let input = span.input;
        if input.players[2..]
            .iter()
            .any(|player| player.buttons != 0 || player.dpad != 0)
        {
            bail!("cartridge NES TAS verification supports players 1 and 2 only");
        }
        if input.zapper.enabled
            || input.zapper.trigger
            || input.zapper.hit
            || input.zapper.screen_pos.is_some()
        {
            bail!("cartridge NES TAS verification does not support Zapper input");
        }
        if input.tilt_x_bits != 0 || input.tilt_y_bits != 0 {
            bail!("cartridge NES TAS verification does not support tilt input");
        }
        if input.camera != TasCameraInput::None {
            bail!("cartridge NES TAS verification does not support camera input");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use zeff_emu_common::replay::{ReplayPlayer, ReplayStartMetadata};

    use crate::tas_project::{TasBranch, TasControllerInput, TasInputFrame, TasInputSpan};

    use super::*;

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_directory() -> Result<TestDirectory> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("zeff_tas_cli_{}_{}", std::process::id(), suffix));
        std::fs::create_dir(&path)?;
        Ok(TestDirectory(path))
    }

    fn executable_nes_rom() -> Vec<u8> {
        let mut rom = vec![0; 16 + 0x4000 + 0x2000];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg] = 0xA9;
        rom[prg + 1] = 0x42;
        rom[prg + 2] = 0x85;
        rom[prg + 3] = 0x00;
        rom[prg + 4] = 0x4C;
        rom[prg + 5] = 0x04;
        rom[prg + 6] = 0x80;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn project_for_rom(rom_path: &Path, rom: &[u8], frames: u64) -> Result<TasProject> {
        let backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            rom_path,
            rom_path,
            Some(rom.to_vec()),
            BackendLoadConfig::default(),
        )?
        .backend;
        let start_state = backend.encode_state_bytes()?;
        let identity = nes_cartridge_identity(&backend, rom, &start_state)?;
        let project = TasProject {
            project_id: "cli-verification".to_owned(),
            source_replay_sha256: None,
            identity,
            start_state,
            replay_start: ReplayStartMetadata::default(),
            edit_generation: 0,
            rerecord_count: 0,
            active_branch_id: "main".to_owned(),
            project_comment: String::new(),
            branches: vec![TasBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                comment: String::new(),
                parent: None,
                frame_count: frames,
                input_spans: vec![TasInputSpan {
                    start: 0,
                    length: frames,
                    input: TasInputFrame {
                        players: [
                            TasControllerInput {
                                buttons: 1,
                                dpad: 0,
                            },
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                            TasControllerInput::default(),
                        ],
                        ..TasInputFrame::default()
                    },
                }],
                events: Vec::new(),
                verification: None,
            }],
            markers: Vec::new(),
            annotations: Vec::new(),
            assets: BTreeMap::new(),
        };
        project.validate()?;
        Ok(project)
    }

    #[test]
    fn native_cli_verifies_saves_then_exports_with_loader_owned_identity() -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let project_path = directory.0.join("movie.ztas");
        let export_path = directory.0.join("movie.zrpl");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 301)?.save_atomic(&project_path)?;

        run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                tas_export_path: Some(export_path.clone()),
                ..HeadlessOptions::default()
            },
        )?;

        let saved = TasProject::load(&project_path)?;
        assert!(saved.verification_is_current("main")?);
        let verification = saved.branches[0]
            .verification
            .as_ref()
            .expect("verification should be persisted");
        assert_eq!(
            verification
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.cursor)
                .collect::<Vec<_>>(),
            vec![300]
        );
        let replay = ReplayPlayer::load(&export_path)?;
        assert_eq!(replay.total_frames(), 301);
        assert_eq!(replay.metadata().checkpoints.len(), 1);
        assert_eq!(
            replay.metadata().final_state_sha256,
            verification.final_state_sha256.map(|digest| digest.0)
        );
        Ok(())
    }

    #[test]
    fn native_cli_rejects_media_change_without_mutating_project() -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let project_path = directory.0.join("movie.ztas");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        let before = std::fs::read(&project_path)?;
        let mut changed = rom;
        changed[16] ^= 1;
        std::fs::write(&rom_path, changed)?;

        let result = run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(project_path)?, before);
        Ok(())
    }

    #[test]
    fn native_cli_rejects_legacy_or_nonstandard_controller_start_state_transactionally()
    -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;

        for name in ["legacy", "oversized", "zapper"] {
            let mut project = project_for_rom(&rom_path, &rom, 1)?;
            if name == "legacy" {
                project.start_state[8..12].copy_from_slice(&10_u32.to_le_bytes());
            } else if name == "oversized" {
                project.start_state = Vec::new();
                project
                    .start_state
                    .extend_from_slice(&zeff_nes_core::save_state::NES_SAVE_STATE_MAGIC);
                project.start_state.extend_from_slice(
                    &zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes(),
                );
                project
                    .start_state
                    .extend_from_slice(&u32::MAX.to_le_bytes());
            } else {
                let mut backend = load_backend_from_rom_source(
                    ActiveSystem::Nes,
                    &rom_path,
                    &rom_path,
                    Some(rom.clone()),
                    BackendLoadConfig::default(),
                )
                .expect("NES backend should load")
                .backend;
                backend.set_zapper_state(true, false, false, None);
                project.start_state = backend.encode_state_bytes().expect("state should encode");
            }
            project.identity.start_state_sha256 = TasDigest::from_bytes(&project.start_state);
            let project_path = directory.0.join(format!("{name}.ztas"));
            project.validate()?;
            project.save_atomic(&project_path)?;
            let before = std::fs::read(&project_path)?;

            let result = run_tas_project_headless(
                &rom_path,
                Vec::new(),
                &HeadlessOptions {
                    tas_project_path: Some(project_path.clone()),
                    ..HeadlessOptions::default()
                },
            );
            assert!(result.is_err(), "{name} start state should be rejected");
            assert_eq!(std::fs::read(project_path)?, before);
        }
        Ok(())
    }

    #[test]
    fn native_cli_rejects_non_direct_media_without_mutating_project() -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let disguised_path = directory.0.join("game.fds");
        let project_path = directory.0.join("movie.ztas");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        std::fs::write(&disguised_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        let before = std::fs::read(&project_path)?;

        let result = run_tas_project_headless(
            &disguised_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(project_path)?, before);
        Ok(())
    }

    #[test]
    fn bounded_nes_media_reader_rejects_oversized_file() -> Result<()> {
        let directory = test_directory()?;
        let path = directory.0.join("oversized.nes");
        std::fs::File::create(&path)?.set_len(MAX_NES_CARTRIDGE_BYTES + 1)?;
        assert!(read_nes_cartridge_bounded(&path).is_err());
        Ok(())
    }

    #[test]
    fn canonical_nes_tas_loader_ignores_battery_sidecar() -> Result<()> {
        let directory = test_directory()?;
        let isolated_path = directory.0.join("isolated.nes");
        let baseline_path = directory.0.join("baseline.nes");
        let mut rom = executable_nes_rom();
        rom[6] = 0x02;
        rom[7] = 0x10;
        rom[8] = 1;
        std::fs::write(&isolated_path, &rom)?;
        std::fs::write(&baseline_path, &rom)?;

        let mut persistent_source = zeff_nes_core::emulator::Emulator::new(
            &rom,
            zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE,
        )?;
        persistent_source.load_persistent_data(&vec![0xA5; 8 * 1024])?;
        let sidecar = persistent_source
            .dump_persistent_data()
            .context("battery fixture should expose persistent data")?;
        std::fs::write(isolated_path.with_extension("sav"), sidecar)?;

        let baseline = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &baseline_path,
            &baseline_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )?
        .backend
        .encode_state_bytes()?;
        let isolated = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &isolated_path,
            &isolated_path,
            Some(rom),
            BackendLoadConfig {
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend
        .encode_state_bytes()?;
        assert_eq!(isolated, baseline);
        Ok(())
    }

    #[test]
    fn nes_cartridge_scope_rejects_every_unowned_timeline_domain() -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let rom = executable_nes_rom();
        let base = project_for_rom(&rom_path, &rom, 1)?;

        let mut cases = Vec::new();
        let mut project = base.clone();
        project.branches[0].input_spans[0].input.players[2].buttons = 1;
        cases.push(project);
        let mut project = base.clone();
        project.branches[0].input_spans[0].input.zapper.enabled = true;
        cases.push(project);
        let mut project = base.clone();
        project.branches[0].input_spans[0].input.tilt_x_bits = 1;
        cases.push(project);
        let mut project = base.clone();
        project.branches[0].input_spans[0].input.camera = TasCameraInput::Blob(TasDigest([1; 32]));
        cases.push(project);
        let mut project = base.clone();
        project.branches[0]
            .events
            .push(zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 });
        cases.push(project);
        let mut project = base;
        project.replay_start.wonder_swan_link_tick = Some(0);
        cases.push(project);

        for project in cases {
            assert!(validate_nes_cartridge_project_scope(&project, "main").is_err());
        }
        Ok(())
    }

    #[test]
    fn export_failure_leaves_durable_verification() -> Result<()> {
        let directory = test_directory()?;
        let rom_path = directory.0.join("game.nes");
        let project_path = directory.0.join("movie.ztas");
        let export_path = directory.0.join("occupied.zrpl");
        let rom = executable_nes_rom();
        std::fs::write(&rom_path, &rom)?;
        project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
        std::fs::write(&export_path, b"keep")?;

        let result = run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                tas_export_path: Some(export_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err());
        assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
        assert_eq!(std::fs::read(export_path)?, b"keep");
        Ok(())
    }
}
