use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, NesTraceOrigin, NesTraceWrite,
};
use zeff_emu_common::time::MasterTicks;

use super::Bus;
use crate::hardware::apu::AUDIO_TRACE_CPU_ORIGIN;

impl Bus {
    pub(crate) fn begin_audio_trace_instruction(&mut self, pc: u16) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_instruction = Some((pc, self.audio_trace_source(pc)));
            self.audio_trace_non_instruction = false;
        }
    }

    pub(crate) fn end_audio_trace_instruction(&mut self) {
        self.audio_trace_instruction = None;
        self.audio_trace_non_instruction = false;
    }

    pub(crate) fn begin_audio_trace_non_instruction(&mut self) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_non_instruction = true;
        }
    }

    pub(super) fn audio_trace_source(&self, address: u16) -> AudioTraceSource {
        if address < 0x2000 {
            return AudioTraceSource::WorkRam {
                offset: u32::from(address & 0x07ff),
            };
        }
        if let Some(offset) = self.cartridge.cpu_rom_offset(address) {
            return AudioTraceSource::CartridgeRom {
                offset: offset as u64
                    + 16
                    + if self.cartridge.header().has_trainer {
                        512
                    } else {
                        0
                    },
                bit_reversed: false,
            };
        }
        if (0x6000..=0x7fff).contains(&address) {
            AudioTraceSource::Unknown
        } else {
            AudioTraceSource::Unmapped
        }
    }

    pub(super) fn record_audio_event(&mut self, at: Option<MasterTicks>, mut write: NesTraceWrite) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        let Some(cycle) = at.and_then(|at| at.get().checked_sub(AUDIO_TRACE_CPU_ORIGIN)) else {
            self.audio_trace
                .invalidate(AudioTraceInvalidation::ExternalMutation);
            return;
        };
        if self.audio_trace_non_instruction
            && let NesTraceWrite::StatusRead { origin, .. } = &mut write
            && *origin == NesTraceOrigin::Cpu
        {
            *origin = NesTraceOrigin::CpuNonInstruction;
        }
        let autonomous = matches!(
            write,
            NesTraceWrite::DmcFetch { .. }
                | NesTraceWrite::StatusRead {
                    origin: NesTraceOrigin::Dma | NesTraceOrigin::CpuNonInstruction,
                    ..
                }
        );
        let (pc, instruction_source) = if autonomous {
            (0, AudioTraceSource::Unknown)
        } else if let Some(instruction) = self.audio_trace_instruction {
            instruction
        } else {
            self.audio_trace
                .invalidate(AudioTraceInvalidation::ExternalMutation);
            return;
        };
        self.audio_trace.record(AudioTraceEvent {
            cycle,
            pc: u32::from(pc),
            instruction_source,
            write,
        });
    }
}
