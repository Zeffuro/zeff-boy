use super::*;
use zeff_md_core::emulator::rollback::MdRollbackConfig;

const DOMAIN: &[u8] = b"ZeffNetplay-MD-factory-concurrent/owned-original/MDlogical-local-v1/48000/f32LE/confirmed-PCM/discard-restore/v1";
fn current(backend: &EmuBackend) -> Result<&crate::emu_backend::md::MdBackend> {
    let EmuBackend::Md(md) = backend else {
        anyhow::bail!("MD backend required");
    };
    Ok(md)
}
pub(super) fn identity(
    backend: &EmuBackend,
    build: [u8; 32],
    delay: InputDelay,
) -> Result<Identity> {
    let md = current(backend)?;
    let load = md.netplay_admission()?;
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(super::super::md_input::INPUT_DOMAIN);
    hash.update(load.config.identity_bytes());
    hash.update(load.policy);
    hash.update(delay.frames().to_le_bytes());
    hash.update(zeff_netplay::rollback::PREDICTION_WINDOW.to_le_bytes());
    let config = hash.finalize().into();
    Ok(Identity {
        build,
        build_info: super::super::compatibility::describe(backend, false),
        source: load.source,
        effective: backend.rom_hash(),
        media_len: load.len,
        config,
        initial: logical(load.hashes.logical(), 0, config),
        persistent: load.persistent,
        state_format: 1,
    })
}
fn logical(core: [u8; 32], frame: u64, config: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"ZeffNetplay-MD-checkpoint-logical-v1");
    hash.update(config);
    hash.update(frame.to_le_bytes());
    hash.update(core);
    hash.finalize().into()
}
pub(super) fn checkpoint(
    backend: &EmuBackend,
    frame: u64,
    audio: &[f32],
    config: [u8; 32],
) -> Result<Message> {
    let md = current(backend)?;
    md.validate_netplay_frame(frame)?;
    let core = MdRollbackConfig::capture(md.core())?.boundary_hashes(md.core())?;
    let mut pcm = Sha256::new();
    for sample in audio {
        pcm.update(sample.to_bits().to_le_bytes());
    }
    Ok(Message::Checkpoint {
        frame,
        logical: logical(core.logical(), frame, config),
        video: Sha256::digest(md.core().framebuffer()).into(),
        audio: pcm.finalize().into(),
        persistent: md.netplay_persistent_hash()?,
    })
}
