use super::Bus;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, WonderSwanTraceOrigin, WonderSwanTraceWrite,
};

impl Bus {
    pub(crate) fn begin_audio_trace_instruction(&mut self, pc: u32) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        let source = if let Some(offset) = self.cartridge.rom_offset_for_address(pc) {
            AudioTraceSource::CartridgeRom {
                offset: offset as u64,
                bit_reversed: false,
            }
        } else if (pc as usize) < self.ram.len() {
            AudioTraceSource::WorkRam { offset: pc }
        } else if (0x10000..=0x1ffff).contains(&pc)
            && self.cartridge.footer().save_kind.is_sram()
            && !self.cartridge.save_data().is_empty()
        {
            let offset = (usize::from(self.cartridge.ram_bank()) * 0x10000 + pc as usize - 0x10000)
                % self.cartridge.save_data().len();
            AudioTraceSource::CartridgeRam {
                offset: offset as u32,
            }
        } else {
            AudioTraceSource::Unmapped
        };
        self.audio_trace_context = (pc, source);
        self.audio_trace_origin = WonderSwanTraceOrigin::Cpu;
    }

    pub(crate) fn end_audio_trace_instruction(&mut self) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_context = (0, AudioTraceSource::Unknown);
            self.audio_trace_origin = WonderSwanTraceOrigin::Cpu;
        }
    }

    pub(crate) fn audio_trace_interrupt(&mut self) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_context = (0, AudioTraceSource::Unknown);
            self.audio_trace_origin = WonderSwanTraceOrigin::CpuInterrupt;
        }
    }

    pub(super) fn trace_audio_register(&mut self, port: u16, value: u8) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(WonderSwanTraceWrite::Register {
                port,
                value,
                origin: self.audio_trace_origin,
            });
        }
    }

    pub(super) fn trace_wave_ram(&mut self, address: u16, value: u8) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(WonderSwanTraceWrite::WaveRam {
                address,
                value,
                origin: self.audio_trace_origin,
            });
        }
    }

    pub(super) fn trace_sound_dma(&mut self, port: u16, value: u8) {
        if self.audio_trace.is_enabled() {
            self.audio_trace.record(AudioTraceEvent {
                cycle: self.cycles,
                pc: 0,
                instruction_source: AudioTraceSource::Unknown,
                write: WonderSwanTraceWrite::Register {
                    port,
                    value,
                    origin: WonderSwanTraceOrigin::SoundDma,
                },
            });
        }
    }

    fn record_audio_write(&mut self, write: WonderSwanTraceWrite) {
        let (pc, instruction_source) = self.audio_trace_context;
        self.audio_trace.record(AudioTraceEvent {
            cycle: self.cycles,
            pc,
            instruction_source,
            write,
        });
    }
}
