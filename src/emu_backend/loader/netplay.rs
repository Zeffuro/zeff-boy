use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn capture(
    backend: &mut EmuBackend,
    source: &Path,
    rom: &Path,
    from_path: bool,
    config: &BackendLoadConfig,
    hash: Option<[u8; 32]>,
    len: usize,
    unmodified: bool,
) -> anyhow::Result<()> {
    if let EmuBackend::Pce(pce) = backend {
        if config.pce_netplay {
            pce.configure_netplay_controllers()?;
        }
        let direct = from_path && source == rom;
        #[cfg(target_arch = "wasm32")]
        let direct = direct || (config.pce_browser_source && source == rom);
        pce.capture_netplay_load_provenance(
            hash.expect("PC Engine source hash must exist"),
            len,
            direct,
            unmodified,
            config.initial_input.unwrap_or_default() == (0, 0),
        )?;
    }
    if let EmuBackend::Sega8(sega) = backend {
        let direct = from_path && source == rom;
        #[cfg(target_arch = "wasm32")]
        let direct = direct || (config.sega8_browser_source && source == rom);
        sega.capture_netplay_load_provenance(
            hash.expect("Sega source hash must exist"),
            len as u64,
            direct,
            unmodified,
            config.initial_input.unwrap_or_default() == (0, 0),
        )?;
    }
    Ok(())
}
