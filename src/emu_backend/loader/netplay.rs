use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NetplayRomMedia {
    pub(crate) hash: [u8; 32],
    pub(crate) len: u64,
    pub(crate) policy: [u8; 32],
}

#[cfg(target_arch = "wasm32")]
impl NetplayRomMedia {
    pub(crate) fn browser(bytes: &[u8]) -> Option<Self> {
        (1..=64 * 1024 * 1024).contains(&bytes.len()).then(|| Self {
            hash: zeff_firmware::sha256_bytes(bytes),
            len: bytes.len() as u64,
            policy: [0; 32],
        })
    }
}

fn authenticated_media(
    system: ActiveSystem,
    source: &Path,
    rom: &Path,
    from_path: bool,
    hash: [u8; 32],
    len: usize,
) -> bool {
    let extension = match system {
        ActiveSystem::Nes => "nes",
        ActiveSystem::MasterSystem => "sms",
        ActiveSystem::Sg1000 => "sg",
        ActiveSystem::Pce => "pce",
        ActiveSystem::WonderSwan if has_extension(rom, "wsc") => "wsc",
        ActiveSystem::WonderSwan => "ws",
        _ => return false,
    };
    if !(1..=64 * 1024 * 1024).contains(&len) || !has_extension(rom, extension) {
        return false;
    }
    if from_path && source == rom {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        has_extension(source, "zip")
            && crate::rom_archive::extract_bounded_zip_member(
                source,
                Some(rom),
                extension,
                128 * 1024 * 1024,
                64 * 1024 * 1024,
            )
            .is_ok_and(|selected| {
                selected.bytes.len() == len && zeff_firmware::sha256_bytes(&selected.bytes) == hash
            })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = hash;
        source == rom
            || (has_extension(source, "zip")
                && rom.strip_prefix(source).is_ok_and(|member| {
                    !member.as_os_str().is_empty()
                        && member
                            .components()
                            .all(|part| matches!(part, std::path::Component::Normal(_)))
                }))
    }
}

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
    let authenticated = hash.is_some_and(|hash| {
        authenticated_media(backend.system(), source, rom, from_path, hash, len)
    });
    #[cfg(target_arch = "wasm32")]
    let authenticated = authenticated
        && config
            .netplay_browser_media
            .is_some_and(|media| Some(media.hash) == hash && media.len == len as u64);
    if let EmuBackend::Nes(nes) = backend {
        nes.set_netplay_media(authenticated.then(|| NetplayRomMedia {
            hash: hash.expect("NES source hash must exist"),
            len: len as u64,
            policy: nes_policy::direct(nes.emu.has_battery()),
        }));
    }
    if let EmuBackend::Pce(pce) = backend {
        if config.pce_netplay {
            pce.configure_netplay_controllers()?;
        }
        pce.capture_netplay_load_provenance(
            hash.expect("PC Engine source hash must exist"),
            len,
            authenticated,
            unmodified,
            config.initial_input.unwrap_or_default() == (0, 0),
        )?;
    }
    if let EmuBackend::Sega8(sega) = backend {
        sega.capture_netplay_load_provenance(
            hash.expect("Sega source hash must exist"),
            len as u64,
            authenticated,
            unmodified,
            config.initial_input.unwrap_or_default() == (0, 0),
        )?;
    }
    if let EmuBackend::Ws(ws) = backend {
        ws.capture_netplay_load_provenance(
            hash.expect("WonderSwan source hash must exist"),
            len,
            authenticated,
            unmodified,
            config.initial_input.unwrap_or_default() == (0, 0),
        )?;
    }
    Ok(())
}
