use sha2::{Digest, Sha256};

const NES_CARTRIDGE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0external-state=absent\0";
const NES_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0";

pub(super) fn direct(battery: bool) -> [u8; 32] {
    Sha256::digest(if battery {
        NES_BATTERY_SYNC_CONFIGURATION
    } else {
        NES_CARTRIDGE_SYNC_CONFIGURATION
    })
    .into()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn browser_media(
    system: crate::emu_backend::ActiveSystem,
    source: &std::path::Path,
    rom: &std::path::Path,
    length: usize,
    digest: Option<[u8; 32]>,
) -> Option<([u8; 32], [u8; 32], [u8; 32])> {
    if system != crate::emu_backend::ActiveSystem::Nes
        || source != rom
        || !source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("nes"))
        || !(16..=64 * 1024 * 1024).contains(&length)
    {
        return None;
    }
    Some((digest?, direct(false), direct(true)))
}
