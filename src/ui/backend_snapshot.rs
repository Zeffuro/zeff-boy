use crate::emu_backend::EmuBackend;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};

use super::{
    UiFrameData, collect_emu_snapshot, collect_gba_snapshot, collect_nes_snapshot,
    collect_pce_snapshot, collect_sega8_snapshot, collect_ws_snapshot,
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
        EmuBackend::Pce(pce) => collect_pce_snapshot(pce, snapshot, buffers.memory_page),
        EmuBackend::Gba(gba) => collect_gba_snapshot(&gba.emu, snapshot, buffers),
        EmuBackend::Ws(ws) => collect_ws_snapshot(&ws.emu, snapshot, buffers),
        EmuBackend::Sega8(sega8) => collect_sega8_snapshot(&sega8.emu, snapshot, buffers),
    };
    if snapshot.show_instruction_trace {
        let store = match backend {
            EmuBackend::Gb(gb) => Some(gb.emu.instruction_trace()),
            EmuBackend::Nes(nes) => Some(nes.emu.instruction_trace()),
            EmuBackend::Pce(_) => None,
            EmuBackend::Gba(gba) => Some(gba.emu.instruction_trace()),
            EmuBackend::Ws(ws) => Some(ws.emu.instruction_trace()),
            EmuBackend::Sega8(sega8) => Some(sega8.emu.instruction_trace()),
        };
        data.instruction_trace =
            store.map(|store| collect_instruction_trace(store, snapshot.trace_after_sequence));
    }
    data.core_features = Some(backend.capabilities());
    data
}

fn collect_instruction_trace(
    store: &zeff_emu_common::debug::InstructionTraceStore,
    after_sequence: Option<u64>,
) -> super::InstructionTraceBatch {
    const MAX_BATCH: usize = 2_048;

    let oldest_sequence = store.oldest_sequence();
    let newest_sequence = store.newest_sequence();
    let entries = store.entries_after(after_sequence, MAX_BATCH);
    super::InstructionTraceBatch {
        enabled: store.is_enabled(),
        capacity: store.capacity(),
        retained: store.len(),
        oldest_sequence,
        newest_sequence,
        entries,
    }
}
