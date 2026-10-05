use super::*;
use zeff_sega8_core::hardware::{
    cartridge::Sega8System, region::Sega8Region, timing::Sega8VideoStandard,
};

pub(super) fn identity(
    backend: &EmuBackend,
    build: [u8; 32],
    delay: InputDelay,
) -> Result<Identity> {
    let sega = backend.sega8().context("netplay requires Sega console")?;
    ensure!(
        matches!(
            sega.emu.system(),
            Sega8System::MasterSystem | Sega8System::Sg1000
        ),
        "shared netplay requires SMS or SG-1000"
    );
    let load = sega
        .netplay_load_provenance()
        .context("netplay requires loader-owned media provenance")?;
    ensure!(
        load.direct_source && load.unmodified,
        "netplay requires a direct unmodified Sega ROM"
    );
    ensure!(
        load.source == backend.rom_hash() && load.source_len != 0,
        "netplay source and effective media differ"
    );
    ensure!(
        sega.netplay_persistent_load_known,
        "netplay persistent load outcome is unknown"
    );
    ensure!(load.neutral_input, "netplay requires neutral initial input");
    ensure!(
        load.sample_rate == 48_000 && sega.emu.sample_rate() == 48_000,
        "netplay requires 48000 Hz audio generation"
    );
    ensure!(
        backend.frame_count() == 0 && !backend.is_suspended(),
        "netplay requires an unexecuted running core"
    );
    ensure!(
        load.state == <[u8; 32]>::from(Sha256::digest(backend.encode_state_bytes()?))
            && load.runtime
                == <[u8; 32]>::from(Sha256::digest(sega.emu.encode_rollback_runtime_state())),
        "netplay core state changed after loading"
    );
    ensure!(
        !sega.emu.bus().has_boot_rom() && backend.media_slot_snapshot().is_none(),
        "netplay does not support firmware or removable media"
    );
    let apu = sega.emu.bus().apu();
    ensure!(
        apu.sample_rate() == 48_000
            && apu.sample_generation_enabled()
            && apu.channel_mutes() == [false; 4]
            && apu.buffered_sample_count() == 0,
        "netplay requires full audio generation"
    );
    ensure!(
        !sega.emu.has_debugger_stop_controls() && sega.emu.rom_patches().is_empty(),
        "netplay excludes debugger, traces and cheats"
    );
    let persistent = persistent_hash(backend)?;
    ensure!(
        persistent == load.persistent,
        "netplay persistent state changed after loading"
    );
    let mut config = Sha256::new();
    config.update(b"ZeffNetplay-Sega8/two-pads/pause-OR-edge/48000/stereo-f32LE/runtime1/native12/discard-and-restore/v1");
    config.update([match sega.emu.system() {
        Sega8System::MasterSystem => 0,
        Sega8System::Sg1000 => 1,
        Sega8System::GameGear => unreachable!(),
    }]);
    config.update([match sega.emu.video_standard() {
        Sega8VideoStandard::Ntsc => 0,
        Sega8VideoStandard::Pal => 1,
    }]);
    config.update([match sega.emu.console_region() {
        Sega8Region::Export => 0,
        Sega8Region::Japanese => 1,
        Sega8Region::JapanesePowerBaseConverter => 2,
    }]);
    config.update(sega.emu.bus().mapper().kind().label().as_bytes());
    config.update([u8::from(sega.emu.sg_type_b_ram_extension())]);
    config.update(delay.frames().to_le_bytes());
    config.update(zeff_netplay::rollback::PREDICTION_WINDOW.to_le_bytes());
    let config = config.finalize().into();
    Ok(Identity {
        build,
        build_info: super::super::compatibility::describe(backend, false),
        source: load.source,
        effective: backend.rom_hash(),
        media_len: load.source_len,
        config,
        initial: logical_hash(backend, 0, config)?,
        persistent,
        state_format: zeff_sega8_core::save_state::SAVE_STATE_FORMAT_VERSION,
    })
}
