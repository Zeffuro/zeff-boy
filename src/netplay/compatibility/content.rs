use super::certificate::VerifiedPair;
use crate::emu_backend::EmuBackend;

pub(super) fn permitted(pair: &VerifiedPair, backend: &EmuBackend) -> bool {
    use zeff_nes_core::hardware::cartridge::TimingMode;
    let Some(nes) = backend.nes() else {
        return false;
    };
    let Some(provenance) = backend.nes_tas_load_provenance() else {
        return false;
    };
    let header = nes.emu.cartridge_header();
    let timing = match nes.emu.resolved_timing_mode() {
        TimingMode::Ntsc => Some(0),
        TimingMode::Pal => Some(1),
        TimingMode::Dendy => Some(2),
        TimingMode::MultiRegion => None,
    };
    provenance.load.raw_source_media_sha256 == backend.rom_hash()
        && nes.emu.cartridge_effective_mapper_label() == header.mapper_label()
        && timing.is_some_and(|timing| {
            pair.permits_content(
                backend.rom_hash(),
                header.mapper_id,
                header.submapper_id,
                timing,
                nes.emu.has_portable_rollback_hardware(),
            )
        })
}
