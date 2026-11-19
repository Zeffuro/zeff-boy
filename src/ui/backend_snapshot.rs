use crate::emu_backend::EmuBackend;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};

use super::{
    UiFrameData, collect_coleco_snapshot, collect_emu_snapshot, collect_gba_snapshot,
    collect_md_snapshot, collect_nes_snapshot, collect_pce_snapshot, collect_sega8_snapshot,
    collect_ws_snapshot,
};

pub(crate) fn collect_backend_snapshot(
    backend: &mut EmuBackend,
    snapshot: &SnapshotRequest,
    buffers: ReusableBuffers,
) -> UiFrameData {
    let suspended = backend.is_suspended();
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
        EmuBackend::Md(md) => collect_md_snapshot(md.core(), snapshot, buffers.memory_page),
        EmuBackend::Coleco(coleco) => collect_coleco_snapshot(&coleco.emu, snapshot, buffers),
        EmuBackend::Pce(pce) => collect_pce_snapshot(pce, snapshot, buffers.memory_page),
        EmuBackend::Gba(gba) => collect_gba_snapshot(&gba.emu, snapshot, buffers),
        EmuBackend::Ws(ws) => collect_ws_snapshot(&ws.emu, snapshot, buffers),
        EmuBackend::Sega8(sega8) => collect_sega8_snapshot(&sega8.emu, snapshot, buffers),
    };
    if snapshot.show_disassembler
        && let Some(crate::debug::DisassemblyTarget::RomStorage(offset)) = snapshot.disasm_target
        && !matches!(backend, EmuBackend::Md(_))
    {
        data.disassembly_view = Some(crate::debug::DisassemblyView::unavailable_rom(offset));
    }

    if let EmuBackend::Md(md) = backend {
        md.overlay_pc_breakpoints(&mut data);
        if snapshot.want_debug_info && snapshot.debug_target == crate::debug::DebugTarget::Sound {
            data.md_sound_watch = Some(md.sound_watch_view(snapshot.debug_revision));
        }
        if let Some(view) = &mut data.disassembly_view
            && view.cursor.is_source()
        {
            view.breakpoints.clear();
            view.one_shot_breakpoints.clear();
        }
        if let Some(cpu) = &mut data.cpu_debug
            && data.debug_target == crate::debug::DebugTarget::Primary
        {
            cpu.cpu_state = if suspended { "Suspended" } else { "Running" }.into();
        }
    }
    if snapshot.show_instruction_trace {
        let sound = match &*backend {
            EmuBackend::Md(md) => Some(super::SoundTraceInfo {
                rejection: md.sound_trace_rejection(),
                gap: md.sound_trace_gap(),
            }),
            _ => None,
        };
        let sound_target = snapshot.trace_cursor.target == super::TraceTarget::Sound;
        let stream = match backend {
            EmuBackend::Md(md) if sound_target => md.sound_instruction_trace_stream(),
            EmuBackend::Md(md) => Some(md.instruction_trace_stream()),
            _ => None,
        };
        let store = match backend {
            EmuBackend::Md(md) if sound_target => Some(md.sound_instruction_trace()),
            EmuBackend::Md(md) => Some(md.instruction_trace()),
            EmuBackend::Gb(gb) => Some(gb.emu.instruction_trace()),
            EmuBackend::Nes(nes) => Some(nes.emu.instruction_trace()),
            EmuBackend::Coleco(coleco) => Some(coleco.emu.instruction_trace()),
            EmuBackend::Pce(pce) => Some(pce.instruction_trace()),
            EmuBackend::Gba(gba) => Some(gba.emu.instruction_trace()),
            EmuBackend::Ws(ws) => Some(ws.emu.instruction_trace()),
            EmuBackend::Sega8(sega8) => Some(sega8.emu.instruction_trace()),
        };
        data.instruction_trace = store.map(|store| {
            let mut batch = if stream.is_none() && snapshot.trace_cursor.stream.is_none() {
                collect_instruction_trace(store, snapshot.trace_cursor.after_sequence)
            } else {
                collect_instruction_trace_stream(store, stream, snapshot.trace_cursor)
            };
            batch.sound = sound;
            if sound_target && sound.is_some() {
                batch.target = super::TraceTarget::Sound;
            }
            batch
        });
    }
    data.debug_revision = snapshot.debug_revision;
    data.core_features = Some(backend.capabilities());
    data
}

fn collect_instruction_trace(
    store: &zeff_emu_common::debug::InstructionTraceStore,
    after_sequence: Option<u64>,
) -> super::InstructionTraceBatch {
    collect_instruction_trace_stream(
        store,
        None,
        super::TraceCursor {
            target: crate::ui::TraceTarget::Primary,
            stream: None,
            after_sequence,
        },
    )
}

fn collect_instruction_trace_stream(
    store: &zeff_emu_common::debug::InstructionTraceStore,
    stream: Option<super::TraceStreamId>,
    cursor: super::TraceCursor,
) -> super::InstructionTraceBatch {
    const MAX_BATCH: usize = 2_048;
    let mismatch = stream != cursor.stream;
    let after_sequence = if mismatch {
        None
    } else {
        cursor.after_sequence
    };
    let limit = if mismatch { store.len() } else { MAX_BATCH };
    let oldest_sequence = store.oldest_sequence();
    let newest_sequence = store.newest_sequence();
    let entries = store.entries_after(after_sequence, limit);
    super::InstructionTraceBatch {
        target: crate::ui::TraceTarget::Primary,
        sound: None,
        stream,
        enabled: store.is_enabled(),
        capacity: store.capacity(),
        retained: store.len(),
        oldest_sequence,
        newest_sequence,
        entries,
    }
}

#[cfg(test)]
mod trace_stream_tests;

#[cfg(test)]
#[path = "backend_snapshot/trace_tests.rs"]
mod trace_tests;
