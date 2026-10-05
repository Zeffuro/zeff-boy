use super::*;

pub(super) fn identity(
    backend: &EmuBackend,
    build: [u8; 32],
    delay: InputDelay,
) -> Result<Identity> {
    let EmuBackend::Ws(ws) = backend else {
        anyhow::bail!("netplay requires WonderSwan");
    };
    let load = ws
        .netplay_load_provenance()
        .context("netplay requires loader-owned media provenance")?;
    ensure!(
        ws.tas_load_provenance()
            .is_some_and(|provenance| provenance.load.persistent_load
                != crate::emu_backend::ws::WsTasPersistentLoadOutcome::Unknown),
        "netplay persistent load outcome is unknown"
    );
    ensure!(
        load.authenticated_source && load.unmodified,
        "netplay requires an authenticated unmodified cartridge"
    );
    ensure!(
        load.source == backend.rom_hash() && load.source_len != 0,
        "netplay source and effective media differ"
    );
    ensure!(load.neutral_input, "netplay requires neutral initial input");
    ensure!(
        load.sample_rate == 48_000 && backend.frame_count() == 0 && !backend.is_suspended(),
        "netplay requires a fresh core with 48000 Hz audio"
    );
    ws.validate_netplay_boundary()?;
    ensure!(
        load.config == <[u8; 32]>::from(Sha256::digest(ws.netplay_config_bytes())),
        "netplay hardware configuration changed after loading"
    );
    ensure!(
        backend.media_slot_snapshot().is_none() && backend.replay_metadata().firmware.is_empty(),
        "netplay excludes firmware and removable media"
    );
    let pair = ws.netplay_initial_pair_checksum()?;
    ensure!(
        load.state == <[u8; 32]>::from(Sha256::digest(backend.encode_state_bytes()?))
            && load.runtime == <[u8; 32]>::from(Sha256::digest(ws.netplay_runtime_state_bytes()?))
            && load.pair_checksum == pair,
        "netplay core state changed after loading"
    );
    let persistent = persistent_hash(backend)?;
    ensure!(
        persistent == load.persistent,
        "netplay persistent state changed"
    );
    let mut config = Sha256::new();
    config.update(b"ZeffNetplay-WS/replicated-pair/ws11/bus-boundary1/fixed-world-frames/48000/runtime1/discard-and-restore/v1");
    config.update(ws.netplay_config_bytes());
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
        initial: logical_hash(pair, 0, config),
        persistent,
        state_format: u32::from(zeff_ws_core::save_state::SAVE_STATE_FORMAT_VERSION),
    })
}

pub(super) fn logical_hash(pair: [u8; 32], frame: u64, config: [u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"ZeffNetplay-WS-paired-runtime-v1");
    digest.update(config);
    digest.update(frame.to_le_bytes());
    digest.update(pair);
    digest.finalize().into()
}
