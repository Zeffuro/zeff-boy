use std::path::Path;

use anyhow::Context;
use zeff_emu_common::memory::{
    MemoryRegionDescriptor, MemoryRegionKind, MemoryRegionView, resolve_memory_region,
};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_pce_core::hardware::{
    CDROM2_BRAM_LEN, MEMORY_BASE128_RAM_LEN, POPULOUS_HUCARD_RAM_LEN, PadButtons,
    PceArcadeCardMode, PceControllerMode, PceHuCardBoard, PceMemoryBaseMode, VCE_PALETTE_COLORS,
    VDC_SATB_WORDS, VDC_VRAM_BYTES,
};

use super::{
    ARCADE_CARD_RAM_REGION, BACKEND_STATE_MAGIC, BACKEND_STATE_VERSION, MAX_CORE_STATE_BYTES,
    MEMORY_BASE128_REGION, PCE_AUDIO_CHANNELS, PceBackend, append_words_le, memory_base128_path,
};
use crate::audio_tooling::{AudioSemanticFrame, AudioTopology};
use crate::emu_core_trait::EmulatorCore;

impl EmulatorCore for PceBackend {
    fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    fn state_restores_framebuffer(&self) -> bool {
        true
    }

    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.machine.drain_audio_samples_into(buf);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.machine.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.machine.set_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        self.machine.set_channel_mutes(mutes);
    }

    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_pad_input(buttons_pressed, dpad_pressed);
    }

    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Two,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p3(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Three,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p4(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Four,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_input_p5(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_multitap_pad_input(
            zeff_pce_core::hardware::MultitapPort::Five,
            buttons_pressed,
            dpad_pressed,
        );
    }

    fn set_pce_mouse_state(
        &mut self,
        mode: PceControllerMode,
        delta_x: i16,
        delta_y: i16,
        buttons_pressed: u8,
    ) {
        self.update_controller_mode(mode);
        self.mouse_host_buttons = super::map_pad_buttons(buttons_pressed, 0);
        if let Some(mouse) = self.machine.devices_mut().controller_mut().mouse_mut() {
            mouse.set_buttons(self.mouse_host_buttons);
            mouse.accumulate_motion(delta_x, delta_y);
        }
    }

    fn set_pce_memory_base_mode(&mut self, mode: PceMemoryBaseMode) {
        self.update_memory_base_mode(mode);
    }

    fn is_suspended(&self) -> bool {
        self.machine.faulted() || self.machine.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        let memory_base_path = memory_base128_path();
        self.flush_persistent_data(&memory_base_path)
    }

    fn save_ram_kind(&self) -> SaveRamKind {
        match self.machine.hucard_board() {
            PceHuCardBoard::Populous => SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
                if self.cdrom2().is_some() =>
            {
                SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
            }
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce
                if self
                    .machine
                    .devices()
                    .controller()
                    .memory_base128()
                    .is_connected() =>
            {
                SaveRamKind::known_battery_backed(MEMORY_BASE128_RAM_LEN)
            }
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce => SaveRamKind::none(),
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3 => SaveRamKind::none(),
        }
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            self.pending_runtime_fault.is_none(),
            "faulted PC Engine backends cannot be saved"
        );
        let core_state = zeff_pce_core::hardware::save_state::encode_state(&self.machine)
            .context("failed to encode PC Engine core state")?;
        let mut writer = StateWriter::with_capacity(core_state.len() + 32);
        writer.write_bytes(BACKEND_STATE_MAGIC);
        writer.write_u32(BACKEND_STATE_VERSION);
        writer.write_u64(self.frame_count);
        writer.write_u8(self.mouse_host_buttons.bits());
        writer.write_vec(&core_state);
        Ok(writer.into_bytes())
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let mut reader = StateReader::new(&bytes);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        anyhow::ensure!(
            &magic == BACKEND_STATE_MAGIC,
            "not a valid PC Engine backend save-state"
        );
        let version = reader.read_u32()?;
        anyhow::ensure!(
            version == BACKEND_STATE_VERSION,
            "unsupported PC Engine backend save-state version {version}"
        );
        let frame_count = reader.read_u64()?;
        let mouse_host_buttons = PadButtons::from_bits_retain(reader.read_u8()?);
        let core_state = reader.read_vec(MAX_CORE_STATE_BYTES)?;
        anyhow::ensure!(
            reader.is_exhausted(),
            "PC Engine backend save-state has unexpected trailing data"
        );

        zeff_pce_core::hardware::save_state::decode_state(&mut self.machine, &core_state)
            .context("failed to decode PC Engine core state")?;
        self.frame_count = frame_count;
        self.mouse_host_buttons = mouse_host_buttons;
        self.pce_controller_mode = match self.machine.devices().controller().device() {
            zeff_pce_core::hardware::ControllerDevice::Disconnected => PceControllerMode::Automatic,
            zeff_pce_core::hardware::ControllerDevice::TwoButton(_) => PceControllerMode::TwoButton,
            zeff_pce_core::hardware::ControllerDevice::SixButton(_) => PceControllerMode::SixButton,
            zeff_pce_core::hardware::ControllerDevice::Multitap(_) => PceControllerMode::Multitap,
            zeff_pce_core::hardware::ControllerDevice::Mouse(_) => PceControllerMode::Mouse,
        };
        self.pce_memory_base_mode = if self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected()
        {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        };
        self.pce_arcade_card_mode = if self.machine.devices().arcade_card().is_some() {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        };
        self.pending_runtime_fault = None;
        self.memory_base_force_flush = self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected();
        self.project_presented_frame();
        Ok(())
    }

    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        let supergrafx = self.machine.devices().supergrafx_video().is_some();
        let mut video_ram = MemoryRegionDescriptor::video_ram(self.video_ram_len());
        let mut oam = MemoryRegionDescriptor::oam(self.oam_len());
        if supergrafx {
            video_ram.view = MemoryRegionView::Aggregate;
            oam.view = MemoryRegionView::Aggregate;
        }
        let mut regions = vec![
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(self.machine.mapped_work_ram().len()),
            video_ram,
            MemoryRegionDescriptor::palette_ram(self.palette_ram_len()),
            oam,
            MemoryRegionDescriptor::framebuffer(self.framebuffer.len()),
        ];
        if self.cdrom2().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(CDROM2_BRAM_LEN));
        } else if self.machine.hucard_ram().is_some() {
            regions.insert(2, MemoryRegionDescriptor::save_ram(POPULOUS_HUCARD_RAM_LEN));
        }
        if self
            .machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected()
        {
            regions.insert(regions.len() - 1, MEMORY_BASE128_REGION);
        }
        if self.machine.devices().arcade_card().is_some() {
            regions.insert(regions.len() - 1, ARCADE_CARD_RAM_REGION);
        }
        regions
    }

    fn system_ram_len(&self) -> usize {
        self.machine.mapped_work_ram().len()
    }

    fn video_ram_len(&self) -> usize {
        let vdc_count = 1 + usize::from(self.machine.devices().supergrafx_video().is_some());
        VDC_VRAM_BYTES * vdc_count
    }

    fn palette_ram_len(&self) -> usize {
        VCE_PALETTE_COLORS * size_of::<u16>()
    }

    fn oam_len(&self) -> usize {
        let vdc_count = 1 + usize::from(self.machine.devices().supergrafx_video().is_some());
        VDC_SATB_WORDS * size_of::<u16>() * vdc_count
    }

    fn supports_audio(&self) -> bool {
        true
    }

    fn supports_cheats(&self) -> bool {
        true
    }

    fn apply_ram_cheats(&mut self, cheats: &[crate::cheats::CheatPatch]) {
        zeff_pce_core::hardware::apply_pce_cheats(&mut self.machine, cheats);
    }

    fn debug_suspend(&mut self) {
        self.machine.debug_suspend();
    }

    fn supports_save_states(&self) -> bool {
        true
    }

    fn supports_guest_calls(&self) -> bool {
        true
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    fn supports_symbol_loading(&self) -> bool {
        true
    }

    fn supports_execution_controls(&self) -> bool {
        true
    }

    fn supports_opcode_history(&self) -> bool {
        true
    }

    fn audio_semantic_frame(&self) -> Option<AudioSemanticFrame> {
        Some(super::pce_audio_semantic_frame(
            self.frame_count,
            self.machine.devices().psg().channels(),
        ))
    }

    fn audio_topology(&self) -> Option<AudioTopology> {
        Some(AudioTopology {
            generation: 1,
            channels: PCE_AUDIO_CHANNELS,
        })
    }

    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let region = resolve_memory_region(&self.memory_regions(), id_or_alias)
            .ok_or_else(|| anyhow::anyhow!("unknown memory region '{id_or_alias}'"))?;
        match region.kind {
            MemoryRegionKind::SystemRam => {
                out.clear();
                out.extend_from_slice(self.machine.mapped_work_ram());
                Ok(region)
            }
            MemoryRegionKind::VideoRam => {
                out.clear();
                append_words_le(out, self.machine.devices().vdc().vram());
                if let Some(video) = self.machine.devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().vram());
                }
                Ok(region)
            }
            MemoryRegionKind::PaletteRam => {
                out.clear();
                for color in self.machine.devices().vce().palette() {
                    out.extend_from_slice(&color.raw().to_le_bytes());
                }
                Ok(region)
            }
            MemoryRegionKind::Oam => {
                out.clear();
                append_words_le(out, self.machine.devices().vdc().satb());
                if let Some(video) = self.machine.devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().satb());
                }
                Ok(region)
            }
            MemoryRegionKind::ExternalWorkRam if region.id == ARCADE_CARD_RAM_REGION.id => {
                out.clear();
                out.extend_from_slice(
                    self.machine
                        .devices()
                        .arcade_card()
                        .expect("Arcade Card RAM region requires Arcade Card hardware")
                        .ram(),
                );
                Ok(region)
            }
            MemoryRegionKind::Framebuffer => {
                out.clear();
                out.extend_from_slice(&self.framebuffer);
                Ok(region)
            }
            MemoryRegionKind::SaveRam => {
                out.clear();
                if region.id == MEMORY_BASE128_REGION.id {
                    out.extend_from_slice(
                        self.machine.devices().controller().memory_base128().ram(),
                    );
                } else if let Some(cdrom) = self.cdrom2() {
                    out.extend_from_slice(cdrom.bram());
                } else {
                    out.extend_from_slice(
                        self.machine
                            .hucard_ram()
                            .expect("save RAM region requires HuCard or CD backup RAM"),
                    );
                }
                Ok(region)
            }
            MemoryRegionKind::CpuAddressSpace => {
                anyhow::bail!("CPU address space is debugger-addressable, not copyable")
            }
            _ => anyhow::bail!(
                "memory region '{}' is not available for PC Engine",
                region.id
            ),
        }
    }

    fn take_runtime_fault(&mut self) -> Option<String> {
        self.pending_runtime_fault.take()
    }
}
