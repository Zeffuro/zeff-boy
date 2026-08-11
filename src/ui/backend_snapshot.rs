use crate::emu_backend::EmuBackend;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};

use super::{
    UiFrameData, collect_emu_snapshot, collect_gba_snapshot, collect_nes_snapshot,
    collect_sega8_snapshot, collect_ws_snapshot,
};

pub(crate) fn collect_backend_snapshot(
    backend: &mut EmuBackend,
    snapshot: &SnapshotRequest,
    buffers: ReusableBuffers,
) -> UiFrameData {
    let mut data = match backend {
        EmuBackend::Gb(gb) => collect_emu_snapshot(
            &gb.emu,
            snapshot,
            buffers.vram,
            buffers.oam,
            buffers.memory_page,
        ),
        EmuBackend::Nes(nes) => collect_nes_snapshot(
            &mut nes.emu,
            snapshot,
            buffers.nes_chr,
            buffers.nes_nametable,
            buffers.memory_page,
        ),
        EmuBackend::Gba(gba) => collect_gba_snapshot(&gba.emu, snapshot, buffers),
        EmuBackend::Ws(ws) => collect_ws_snapshot(&ws.emu, snapshot, buffers),
        EmuBackend::Sega8(sega8) => collect_sega8_snapshot(&sega8.emu, snapshot, buffers),
    };
    data.core_features = Some(backend.capabilities());
    data
}
