use super::*;

pub(super) fn identity(
    backend: &EmuBackend,
    build: [u8; 32],
    delay: InputDelay,
) -> Result<Identity> {
    let pce = backend.pce().context("netplay requires PC Engine")?;
    let load = pce
        .netplay_load_provenance()
        .context("netplay requires loader-owned media provenance")?;
    ensure!(
        load.direct_source && load.unmodified,
        "netplay requires a direct unmodified HuCard"
    );
    ensure!(
        load.source == backend.rom_hash() && load.source_len != 0,
        "netplay requires a headerless HuCard"
    );
    ensure!(load.neutral_input, "netplay requires neutral initial input");
    ensure!(
        load.sample_rate == 48_000 && backend.frame_count() == 0 && !backend.is_suspended(),
        "netplay requires a fresh core with 48000 Hz audio"
    );
    pce.validate_netplay_boundary()?;
    ensure!(
        load.config == <[u8; 32]>::from(Sha256::digest(pce.netplay_config_bytes())),
        "netplay hardware configuration changed after loading"
    );
    ensure!(
        backend.media_slot_snapshot().is_none() && backend.replay_metadata().firmware.is_empty(),
        "HuCard netplay excludes firmware and removable media"
    );
    ensure!(
        load.state == <[u8; 32]>::from(Sha256::digest(backend.encode_state_bytes()?))
            && load.runtime == <[u8; 32]>::from(Sha256::digest(pce.netplay_runtime_state_bytes())),
        "netplay core state changed after loading"
    );
    let persistent = persistent_hash(backend)?;
    ensure!(
        persistent == load.persistent,
        "netplay persistent state changed"
    );
    let mut config = Sha256::new();
    config.update(
        b"ZeffNetplay-PCE/two-standard-pads/48000/stereo-f32LE/runtime1/discard-and-restore/v1",
    );
    config.update(pce.netplay_config_bytes());
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
        state_format: zeff_pce_core::hardware::save_state::PCE_SAVE_STATE_FORMAT_VERSION,
    })
}
