use super::Bus;
use crate::hardware::constants::{COPIER_HEADER_SIZE, ROM_PAGE_8K_SIZE, WORK_RAM_START};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, AudioTraceWrite,
};

impl Bus {
    pub(crate) fn begin_audio_trace_instruction(&mut self, pc: u16, cycle: u64) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_instruction = Some((pc, cycle));
        }
    }

    pub(crate) fn end_audio_trace_instruction(&mut self) {
        if self.audio_trace_instruction.is_some() {
            self.audio_trace_instruction = None;
        }
    }

    pub(super) fn record_audio_write(&mut self, write: AudioTraceWrite) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        let Some((pc, cycle)) = self.audio_trace_instruction else {
            self.audio_trace
                .invalidate(AudioTraceInvalidation::ExternalMutation);
            return;
        };
        let instruction_source = self.audio_instruction_source(pc);
        self.audio_trace.record(AudioTraceEvent {
            cycle,
            pc: u32::from(pc),
            instruction_source,
            write,
        });
    }

    fn audio_instruction_source(&self, pc: u16) -> AudioTraceSource {
        if self.boot_rom_read(pc).is_some() {
            return AudioTraceSource::BootRom {
                offset: u64::from(pc),
            };
        }
        if pc >= WORK_RAM_START {
            return if self.work_ram_enabled_for_memory() {
                AudioTraceSource::WorkRam {
                    offset: self.work_ram_offset(pc) as u32,
                }
            } else {
                AudioTraceSource::Unmapped
            };
        }
        if !self.cartridge_enabled_for_memory() {
            return AudioTraceSource::Unmapped;
        }
        let header = if self.cartridge.copier_header_stripped() {
            COPIER_HEADER_SIZE
        } else {
            0
        };
        if let Some((page, offset, bit_reversed)) = self
            .mapper
            .rom_page_8k_mapping(pc, self.cartridge.rom_page_8k_count())
        {
            let offset = (usize::from(page) * ROM_PAGE_8K_SIZE + usize::from(offset))
                % self.cartridge.normalized_len();
            return AudioTraceSource::CartridgeRom {
                offset: (offset + header) as u64,
                bit_reversed,
            };
        }
        if let Some(offset) = self.rom_offset_for_cpu_address(pc) {
            return AudioTraceSource::CartridgeRom {
                offset: (offset + header) as u64,
                bit_reversed: false,
            };
        }
        let offset = if self.mapper.slot2_cartridge_ram_enabled() && pc >= 0x8000 {
            self.sega_mapper_ram_offset(pc)
        } else {
            self.mapper.codemasters_cartridge_ram_offset(pc)
        };
        offset.map_or(AudioTraceSource::Unmapped, |offset| {
            AudioTraceSource::CartridgeRam {
                offset: offset as u32,
            }
        })
    }
}
