use super::super::App;
use crate::debug::ConsoleReadSpace;
use crate::debug::dock::TabDataRequirements;
use crate::emu_thread::{RenderSettings, ReusableBuffers, SnapshotRequest};

impl App {
    pub(super) fn build_snapshot_request(
        &mut self,
        reqs: &TabDataRequirements,
        want_viewer_update: bool,
    ) -> SnapshotRequest {
        let nes_custom_palette = self.nes_custom_palette_for_render();
        let remote_wants_debug = if self.remote_debug_frames_remaining > 0 {
            self.remote_debug_frames_remaining -= 1;
            true
        } else {
            false
        };
        let remote_wants_memory = if self.remote_memory_frames_remaining > 0 {
            self.remote_memory_frames_remaining -= 1;
            true
        } else {
            false
        };
        let remote_wants_graphics = if self.remote_graphics_frames_remaining > 0 {
            self.remote_graphics_frames_remaining -= 1;
            true
        } else {
            false
        };
        let console_read = self.debug_windows.console.pending_read;
        let console_memory_start = console_read
            .filter(|read| read.space == ConsoleReadSpace::Cpu)
            .map(|read| read.start);
        let console_rom_start = console_read
            .filter(|read| read.space == ConsoleReadSpace::Rom)
            .map(|read| read.start);
        let memory_view_start = if let Some(start) = console_memory_start {
            start
        } else if remote_wants_memory {
            self.remote_memory_view_start
                .unwrap_or(self.debug_windows.memory.view_start)
        } else {
            self.debug_windows.memory.view_start
        };

        SnapshotRequest {
            want_debug_info: (reqs.needs_debug_info && want_viewer_update) || remote_wants_debug,
            want_perf_info: (reqs.needs_perf_info && want_viewer_update)
                || self.settings.ui.show_fps
                || remote_wants_debug,
            any_viewer_open: reqs.needs_viewer_data && want_viewer_update,
            any_vram_viewer_open: (reqs.needs_vram && want_viewer_update) || remote_wants_graphics,
            show_oam_viewer: (reqs.needs_oam && want_viewer_update) || remote_wants_graphics,
            show_apu_viewer: reqs.needs_apu && want_viewer_update,
            show_disassembler: reqs.needs_disassembly && want_viewer_update,
            show_rom_info: reqs.needs_rom_info && want_viewer_update,
            show_memory_viewer: (reqs.needs_memory_page && want_viewer_update)
                || remote_wants_memory
                || console_memory_start.is_some(),
            memory_view_start,
            show_rom_viewer: (reqs.needs_rom_page && want_viewer_update)
                || console_rom_start.is_some(),
            show_instruction_trace: reqs.needs_trace && want_viewer_update,
            trace_after_sequence: self.debug_windows.trace.last_sequence,
            rom_view_start: console_rom_start.unwrap_or(self.debug_windows.rom_viewer.view_start),
            last_disasm_pc: self.debug_windows.last_disasm_pc,
            last_disasm_mapping: self.debug_windows.last_disasm_mapping,
            disasm_target: self.debug_windows.disasm_target,
            memory_search: super::search::parse_pending_search(&mut self.debug_windows.memory),
            rom_search: super::search::parse_pending_search(&mut self.debug_windows.rom_viewer),
            render: RenderSettings {
                color_correction: self.settings.video.gb_color_correction,
                color_correction_matrix: self.settings.video.gb_color_correction_matrix,
                dmg_palette_preset: self.settings.video.gb_dmg_palette_preset,
                nes_palette_mode: self.settings.video.nes_palette_mode,
                nes_custom_palette,
                sgb_border_enabled: self.settings.emulation.sgb_border_enabled,
            },
        }
    }

    pub(super) fn take_reusable_buffers(&mut self) -> ReusableBuffers {
        ReusableBuffers {
            audio: self.recycled.audio.take(),
            vram: self.recycled.vram.take(),
            oam: self.recycled.oam.take(),
            memory_page: self.recycled.memory_page.take(),
            nes_chr: self.recycled.nes_chr.take(),
            nes_nametable: self.recycled.nes_nametable.take(),
        }
    }
}
