use std::path::Path;

use crate::audio_tooling::{AudioSemanticFrame, AudioTopology};
use crate::cheats::CheatPatch;
use zeff_emu_common::address::Address;
use zeff_emu_common::memory::{
    ExtendedMemoryRegionSizes, MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region,
    standard_memory_regions_with_extended,
};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::FrameLifecycle;

pub(crate) trait DebuggableEmulator {
    fn add_breakpoint(&mut self, addr: Address);
    fn add_one_shot_breakpoint(&mut self, addr: Address);
    fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64);
    fn set_event_breakpoint(&mut self, event: zeff_emu_common::debug::DebugEvent, enabled: bool);
    fn add_watchpoint_range(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    );
    fn remove_watchpoint(
        &mut self,
        start: Address,
        end: Address,
        wt: zeff_emu_common::debug::WatchType,
    );
    fn remove_breakpoint(&mut self, addr: Address);
    fn toggle_breakpoint(&mut self, addr: Address);
    #[allow(dead_code)]
    fn cpu_peek8(&self, addr: Address) -> u8;
    fn cpu_write8(&mut self, addr: Address, val: u8);
    fn is_cpu_suspended(&self) -> bool;
    fn debug_continue(&mut self);
    fn debug_step(&mut self);
    #[inline]
    fn supports_opcode_history(&self) -> bool {
        false
    }
    #[inline]
    fn set_opcode_log_enabled(&mut self, _enabled: bool) {}

    fn set_instruction_trace_enabled(&mut self, _enabled: bool) {}
    fn set_instruction_trace_capacity(&mut self, _capacity: usize) {}
    fn clear_instruction_trace(&mut self) {}
}

pub(crate) trait EmulatorCore: FrameLifecycle {
    fn framebuffer(&self) -> &[u8];
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>);
    #[allow(dead_code)]
    fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut buf = Vec::new();
        self.drain_audio_samples_into(&mut buf);
        buf
    }
    fn set_sample_rate(&mut self, rate: u32);
    fn set_apu_sample_generation_enabled(&mut self, enabled: bool);
    fn set_apu_channel_mutes(&mut self, mutes: &[bool]);
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8);
    #[inline]
    fn set_pce_mouse_state(
        &mut self,
        _mode: zeff_pce_core::hardware::PceControllerMode,
        _delta_x: i16,
        _delta_y: i16,
        _buttons_pressed: u8,
    ) {
    }
    #[inline]
    fn set_pce_memory_base_mode(&mut self, _mode: zeff_pce_core::hardware::PceMemoryBaseMode) {}
    #[inline]
    fn set_zapper_state(
        &mut self,
        _enabled: bool,
        _trigger: bool,
        _hit: bool,
        _screen_pos: Option<(u16, u16)>,
    ) {
    }
    fn is_suspended(&self) -> bool;
    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>>;
    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>>;
    fn load_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome>;
    #[inline]
    fn state_restores_framebuffer(&self) -> bool {
        false
    }
    fn rom_path(&self) -> &Path;
    fn rom_hash(&self) -> [u8; 32];

    #[inline]
    fn supports_save_states(&self) -> bool {
        false
    }

    #[inline]
    fn supports_state_capture(&self) -> bool {
        self.supports_save_states()
    }

    #[inline]
    fn supports_rewind(&self) -> bool {
        self.supports_state_capture()
    }

    #[inline]
    fn supports_replay(&self) -> bool {
        self.supports_state_capture()
    }

    #[inline]
    fn supports_audio(&self) -> bool {
        false
    }

    #[inline]
    fn supports_cheats(&self) -> bool {
        false
    }

    #[inline]
    fn install_rom_patches(&mut self, _cheats: &[CheatPatch]) {}

    #[inline]
    fn apply_ram_cheats(&mut self, _cheats: &[CheatPatch]) {}

    #[inline]
    fn supports_guest_calls(&self) -> bool {
        self.supports_debugger() && self.supports_state_capture()
    }

    #[inline]
    fn take_runtime_fault(&mut self) -> Option<String> {
        None
    }

    #[inline]
    fn save_ram_kind(&self) -> SaveRamKind {
        SaveRamKind::none()
    }

    #[inline]
    fn system_ram_len(&self) -> usize {
        0
    }

    #[inline]
    fn video_ram_len(&self) -> usize {
        0
    }

    #[inline]
    fn palette_ram_len(&self) -> usize {
        0
    }

    #[inline]
    fn oam_len(&self) -> usize {
        0
    }

    #[inline]
    fn io_registers_len(&self) -> usize {
        0
    }

    #[inline]
    fn external_work_ram_len(&self) -> usize {
        0
    }

    #[inline]
    fn internal_work_ram_len(&self) -> usize {
        0
    }

    #[inline]
    fn supports_debugger(&self) -> bool {
        false
    }

    fn debug_suspend(&mut self);

    #[inline]
    fn supports_execution_controls(&self) -> bool {
        self.supports_debugger()
    }

    #[inline]
    fn supports_symbol_loading(&self) -> bool {
        self.supports_debugger()
    }

    #[inline]
    fn supports_opcode_history(&self) -> bool {
        false
    }

    #[inline]
    fn cpu_address_bits(&self) -> u8 {
        16
    }

    fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        standard_memory_regions_with_extended(
            self.cpu_address_bits(),
            self.system_ram_len(),
            self.video_ram_len(),
            self.save_ram_kind(),
            self.framebuffer().len(),
            ExtendedMemoryRegionSizes {
                external_work_ram_len: self.external_work_ram_len(),
                internal_work_ram_len: self.internal_work_ram_len(),
                palette_ram_len: self.palette_ram_len(),
                oam_len: self.oam_len(),
                io_registers_len: self.io_registers_len(),
            },
        )
    }

    #[allow(dead_code)]
    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias).ok_or_else(|| {
            anyhow::anyhow!("unknown memory region '{id_or_alias}' for this core")
        })?;

        match region.kind {
            MemoryRegionKind::Framebuffer => {
                copy_slice_to_vec(out, self.framebuffer());
                Ok(region)
            }
            MemoryRegionKind::CpuAddressSpace => Err(anyhow::anyhow!(
                "CPU address space is debugger-addressable, not copyable as a finite memory region"
            )),
            _ => Err(anyhow::anyhow!(
                "memory region '{}' is not copyable for this core yet",
                region.id
            )),
        }
    }

    #[inline]
    fn set_input_p2(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}
    #[inline]
    fn set_input_p3(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}
    #[inline]
    fn set_input_p4(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}
    #[inline]
    fn set_input_p5(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}
    #[inline]
    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        None
    }
    #[inline]
    fn audio_topology(&self) -> Option<AudioTopology> {
        None
    }
    #[inline]
    fn rumble_active(&self) -> bool {
        false
    }
    #[inline]
    fn is_mbc7(&self) -> bool {
        false
    }
    #[inline]
    fn is_pocket_camera(&self) -> bool {
        false
    }
}

#[allow(dead_code)]
pub(crate) fn copy_slice_to_vec(out: &mut Vec<u8>, data: &[u8]) {
    out.clear();
    out.extend_from_slice(data);
}

#[allow(dead_code)]
pub(crate) fn copy_optional_region_to_vec(
    out: &mut Vec<u8>,
    data: Option<Vec<u8>>,
    region_id: &str,
) -> anyhow::Result<()> {
    let data = data.ok_or_else(|| anyhow::anyhow!("memory region '{region_id}' is unavailable"))?;
    copy_slice_to_vec(out, &data);
    Ok(())
}
