use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_core_trait::EmulatorCore;
use crate::netplay::md_fixture as guest;
use std::path::Path;
use zeff_emu_common::time::FrameLifecycle;
use zeff_md_core::{ControllerKind, Region, TimingModel};

fn loaded(path: &Path, kind: ControllerKind, publication: bool) -> Result<Box<MdBackend>> {
    let backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            md_region: Some(Region::NtscUs),
            md_timing: TimingModel::Standard,
            md_controller: kind,
            md_load_save_ram: false,
            md_host_persistence: publication,
            sample_rate: Some(48_000),
            ..Default::default()
        },
    )?
    .backend;
    let EmuBackend::Md(md) = backend else {
        anyhow::bail!("normal loader lost MD backend")
    };
    Ok(md)
}

fn seed(path: &Path, kind: ControllerKind) -> Result<MdTasSeed> {
    let md = loaded(path, kind, false)?;
    md.capture_authenticated_tas_seed(&md.encode_state_bytes()?)
}

type LiveObservation = (Vec<u8>, Vec<u8>, MdTasLiveProof, Vec<u32>);

fn observation(md: &MdBackend) -> Result<LiveObservation> {
    Ok((
        md.core().encode_state()?,
        md.core().encode_debugger_checkpoint()?,
        MdTasLiveProof::capture(md.core())?,
        guest::bits(&md.core().clone().drain_audio_samples()),
    ))
}

impl MdBackend {
    fn take_tas_live_pending_audio(&mut self, lease: &MdTasLiveLease) -> Result<Vec<f32>> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &mut self.machine else {
            anyhow::bail!("MD TAS audio requires leased candidate")
        };
        ensure!(state.seed.is_some(), "MD TAS seed is not installed");
        state.session.take_pending_audio()
    }
}

fn begin(md: &mut MdBackend) -> Result<MdTasLiveLease> {
    md.begin_tas_live(false).map_err(|failure| failure.error)
}

#[test]
fn normal_md_live_seed_snapshot_stage_and_commit_preserve_both_pad_outputs_and_original_until_finish()
-> Result<()> {
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        let directory = crate::test_support::test_directory("md-tas-live-commit")?;
        let path = directory.path().join("parallel.md");
        std::fs::write(&path, guest::rom())?;
        let mut live = loaded(&path, kind, true)?;
        live.step_frame();
        live.audio_generation_enabled = Some(true);
        live.rewind_audio_changed.set(true);
        let original_address = live.core() as *const Emulator as usize;
        let original = observation(&live)?;
        assert!(
            original
                .3
                .iter()
                .any(|&sample| f32::from_bits(sample) != 0.0)
        );
        let lease = begin(&mut live)?;
        assert!(!live.take_rewind_audio_change());
        assert!(live.rewind_audio_changed.get());
        assert!(live.advance_tas_live(&lease, [0, 0]).is_err());
        assert!(live.take_tas_live_pending_audio(&lease).is_err());
        live.install_authenticated_tas_seed(&lease, seed(&path, kind)?)?;
        assert_eq!(live.tas_live_load(&lease)?.profile().kind(), kind);
        assert_eq!(live.tas_live_proof(&lease)?.frame(), 0);
        let fresh = live.capture_tas_live(&lease)?;
        let (one, two) = guest::words(kind);
        let mut control = loaded(&path, kind, false)?;
        for index in 0..2 {
            control.set_input(one[index] as u8, (one[index] >> 8) as u8);
            control.set_input_p2(two[index] as u8, (two[index] >> 8) as u8);
            control.step_frame();
            let mut expected = Vec::new();
            control.drain_audio_samples_into(&mut expected);
            let result = live.advance_tas_live(&lease, [one[index], two[index]])?;
            assert!(
                !result.audio.is_empty() && result.audio.iter().all(|sample| sample.is_finite())
            );
            assert!(result.audio.iter().any(|sample| *sample != 0.0));
            assert_eq!(guest::bits(&result.audio), guest::bits(&expected));
        }
        let final_state = observation(&live)?;
        let candidate = live.capture_tas_live(&lease)?;
        live.restore_tas_live(&lease, &fresh)?;
        assert_eq!(live.tas_live_proof(&lease)?.frame(), 0);
        live.restore_tas_live(&lease, &candidate)?;
        assert_eq!(observation(&live)?, final_state);
        assert_eq!(observation(&live)?, observation(&control)?);
        let original_core = live.tas_live_original(&lease)?;
        assert_eq!(original_core as *const Emulator as usize, original_address);
        assert_eq!(original_core.encode_debugger_checkpoint()?, original.1);
        assert!(
            live.prepare_tas_live_promotion(&lease, fresh.proof())
                .is_err()
        );
        assert_eq!(observation(&live)?, final_state);
        let proof = live.tas_live_proof(&lease)?;
        live.prepare_tas_live_promotion(&lease, &proof)?;
        assert!(live.test_normal_core_mut().is_err());
        assert!(!live.host_persistence_enabled());
        let candidate_address = live.core() as *const Emulator as usize;
        assert_eq!(
            live.tas_live_original(&lease)? as *const Emulator as usize,
            original_address
        );
        let mut allocations = MdRetainedAllocations::new();
        live.visit_rollback_core_allocations(&mut allocations)?;
        assert!(allocations.finish()?.requested_payload_bytes() > 0);
        assert!(live.tas_live_metadata_bytes(&lease)? > 0);
        live.finish_tas_live_promotion(&lease, &proof, Ok(()))?;
        assert!(!live.rollback_owned() && live.host_persistence_enabled());
        assert_eq!(live.audio_generation_enabled, Some(true));
        assert!(live.rewind_audio_changed.get());
        assert_eq!(live.core() as *const Emulator as usize, candidate_address);
        assert_eq!(observation(&live)?, observation(&control)?);
        assert!(live.tas_live_proof(&lease).is_err());
        for _ in 0..2 {
            live.step_frame();
            control.step_frame();
            let mut actual = Vec::new();
            let mut expected = Vec::new();
            live.drain_audio_samples_into(&mut actual);
            control.drain_audio_samples_into(&mut expected);
            assert!(actual.iter().any(|sample| *sample != 0.0));
            assert_eq!(guest::bits(&actual), guest::bits(&expected));
            assert_eq!(observation(&live)?, observation(&control)?);
        }
    }
    Ok(())
}

#[test]
fn normal_md_live_failed_promotion_returns_original_and_keeps_restore_fence_until_worker_verifies()
-> Result<()> {
    let directory = crate::test_support::test_directory("md-tas-live-abort")?;
    let path = directory.path().join("parallel.md");
    std::fs::write(&path, guest::rom())?;
    let mut live = loaded(&path, ControllerKind::ThreeButton, true)?;
    live.step_frame();
    let address = live.core() as *const Emulator as usize;
    let original = observation(&live)?;
    let lease = begin(&mut live)?;
    live.install_authenticated_tas_seed(&lease, seed(&path, ControllerKind::ThreeButton)?)?;
    live.advance_tas_live(&lease, [0x0101, 0x080a])?;
    let proof = live.tas_live_proof(&lease)?;
    live.prepare_tas_live_promotion(&lease, &proof)?;
    assert!(
        live.finish_tas_live_promotion(
            &lease,
            &proof,
            Err(anyhow::anyhow!("worker stage refused"))
        )
        .is_err()
    );
    assert!(matches!(live.machine, MdMachine::TasRestoring(_)));
    assert!(live.tas_live_load(&lease).is_err());
    assert_eq!(live.core() as *const Emulator as usize, address);
    assert_eq!(observation(&live)?, original);
    assert!(live.flush_battery_sram().is_err());
    live.step_frame();
    live.set_input(0xff, 0x0f);
    assert_eq!(observation(&live)?, original);
    assert!(
        live.complete_tas_live_original(&lease, Err(anyhow::anyhow!("worker original unverified")))
            .is_err()
    );
    assert!(live.rollback_owned() && !live.host_persistence_enabled());
    assert_eq!(observation(&live)?, original);
    live.complete_tas_live_original(&lease, Ok(()))?;
    assert_eq!(live.core() as *const Emulator as usize, address);
    assert_eq!(observation(&live)?, original);
    assert!(live.host_persistence_enabled());
    assert!(live.complete_tas_live_original(&lease, Ok(())).is_err());
    Ok(())
}

#[test]
fn normal_md_live_rejects_foreign_stale_corrupt_seed_and_debug_ownership_without_mutation()
-> Result<()> {
    let directory = crate::test_support::test_directory("md-tas-live-identity")?;
    let path = directory.path().join("parallel.md");
    let bytes = guest::rom();
    std::fs::write(&path, &bytes)?;
    let kind = ControllerKind::ThreeButton;
    let mut live = loaded(&path, kind, true)?;
    live.step_frame();
    let original = observation(&live)?;
    let lease = begin(&mut live)?;
    let before_seed = live.tas_live_proof(&lease)?;
    assert!(
        live.install_authenticated_tas_seed(&lease, seed(&path, ControllerKind::SixButton)?)
            .is_err()
    );
    assert_eq!(live.tas_live_proof(&lease)?, before_seed);
    live.install_authenticated_tas_seed(&lease, seed(&path, kind)?)?;
    let mut other = loaded(&path, kind, false)?;
    let other_lease = begin(&mut other)?;
    other.install_authenticated_tas_seed(&other_lease, seed(&path, kind)?)?;
    let foreign = other.capture_tas_live(&other_lease)?;
    let current = live.tas_live_proof(&lease)?;
    assert!(live.tas_live_proof(&other_lease).is_err());
    assert!(live.tas_live_load(&other_lease).is_err());
    assert!(live.restore_tas_live(&lease, &foreign).is_err());
    assert_eq!(live.tas_live_proof(&lease)?, current);
    let mut corrupt = live.capture_tas_live(&lease)?;
    corrupt.proof.master = corrupt.proof.master.wrapping_add(1);
    assert!(live.restore_tas_live(&lease, &corrupt).is_err());
    assert_eq!(live.tas_live_proof(&lease)?, current);
    let mut netplay = loaded(&path, kind, false)?;
    let config = MdRollbackConfig::capture(netplay.core())?;
    let raw = netplay.encode_state_bytes()?;
    let persistent = netplay.netplay_persistent_hash()?;
    let initial_field = netplay.core().pacing_field_count();
    let netplay_token = netplay.begin_rollback(&config)?;
    let hashes = netplay.rollback_hashes(&netplay_token)?;
    assert!(live.rollback_hashes(&netplay_token).is_err());
    assert_eq!(netplay.rollback_hashes(&netplay_token)?, hashes);
    netplay.abort_rollback_start(
        &netplay_token,
        &crate::emu_backend::md::MdSessionRestore {
            config: &config,
            hashes: &hashes,
            raw: &raw,
            persistent: &persistent,
            initial_field,
        },
    )?;
    assert!(!netplay.rollback_owned());
    let mut changed = bytes.clone();
    changed[0x3ff] ^= 1;
    std::fs::write(&path, changed)?;
    assert!(live.rebuild_tas_live_candidate(&lease).is_err());
    assert_eq!(live.tas_live_proof(&lease)?, current);
    live.prepare_tas_live_original_return(&lease)?;
    live.complete_tas_live_original(&lease, Ok(()))?;
    assert_eq!(observation(&live)?, original);
    std::fs::write(&path, &bytes)?;
    let new_lease = begin(&mut live)?;
    live.install_authenticated_tas_seed(&new_lease, seed(&path, kind)?)?;
    let new_proof = live.tas_live_proof(&new_lease)?;
    assert!(live.tas_live_proof(&lease).is_err());
    assert!(live.restore_tas_live(&new_lease, &corrupt).is_err());
    assert_eq!(live.tas_live_proof(&new_lease)?, new_proof);
    live.prepare_tas_live_original_return(&new_lease)?;
    live.complete_tas_live_original(&new_lease, Ok(()))?;
    other.prepare_tas_live_original_return(&other_lease)?;
    other.complete_tas_live_original(&other_lease, Ok(()))?;
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        let mut offline = loaded(&path, kind, false)?;
        let config = MdRollbackConfig::capture(offline.core())?;
        let original_address = offline.core() as *const Emulator as usize;
        let original = observation(&offline)?;
        let host = offline.tas_editor_host_witness_for_test()?;
        let persistent = offline.netplay_persistent_hash()?;
        let initial_field = offline.core().pacing_field_count();
        assert!(offline.netplay_admission().is_ok());
        let offline_owner = offline.begin_tas_editor()?;
        assert!(offline.begin_tas_live(false).is_err());
        assert!(offline.netplay_admission().is_err());
        assert!(offline.begin_rollback(&config).is_err());
        assert_eq!(offline.core() as *const Emulator as usize, original_address);
        assert_eq!(observation(&offline)?, original);
        assert_eq!(offline.tas_editor_host_witness_for_test()?, host);
        assert!(matches!(offline.machine, MdMachine::Normal(_)));
        drop(offline_owner);
        assert!(offline.netplay_admission().is_ok());
        let token = offline.begin_rollback(&config)?;
        let hashes = offline.rollback_hashes(&token)?;
        offline.retire_rollback(&token)?;
        offline.abort_rollback_start(
            &token,
            &crate::emu_backend::md::MdSessionRestore {
                config: &config,
                hashes: &hashes,
                raw: &original.0,
                persistent: &persistent,
                initial_field,
            },
        )?;
        assert!(matches!(offline.machine, MdMachine::Normal(_)));
        assert_eq!(offline.core() as *const Emulator as usize, original_address);
        assert_eq!(observation(&offline)?, original);
        assert_eq!(offline.tas_editor_host_witness_for_test()?, host);
    }
    let mut debug = loaded(&path, kind, true)?;
    debug.set_opcode_history_enabled(true);
    debug.step_frame();
    let debug_raw = debug.encode_state_bytes()?;
    let debug_owned = debug.core().encode_debugger_checkpoint()?;
    let debug_pcm = guest::bits(&debug.core().clone().drain_audio_samples());
    let failure = match debug.begin_tas_live(false) {
        Err(failure) => failure,
        Ok(_) => anyhow::bail!("active debugger history was admitted"),
    };
    assert!(failure.fenced_lease.is_none());
    assert!(!debug.rollback_owned() && debug.host_persistence_enabled());
    assert_eq!(debug.encode_state_bytes()?, debug_raw);
    assert_eq!(debug.core().encode_debugger_checkpoint()?, debug_owned);
    assert_eq!(
        guest::bits(&debug.core().clone().drain_audio_samples()),
        debug_pcm
    );
    assert!(debug.tas_load().is_err());
    let fresh = loaded(&path, kind, false)?;
    let mut bad_raw = fresh.encode_state_bytes()?;
    bad_raw[0] ^= 1;
    assert!(fresh.capture_authenticated_tas_seed(&bad_raw).is_err());
    Ok(())
}
