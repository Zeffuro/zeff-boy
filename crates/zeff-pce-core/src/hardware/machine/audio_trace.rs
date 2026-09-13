use super::*;
use crate::hardware::bus::decode_physical_region;
use crate::hardware::constants::{
    PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR, PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR,
};
use crate::hardware::cpu::PHYSICAL_ADDRESS_MASK;
use crate::hardware::psg::{
    PSG_CLOCK_DENOMINATOR, PSG_CLOCK_NUMERATOR, PSG_INTERNAL_MASTER_CLOCK_DIVISOR,
    PSG_MASTER_CLOCK_DIVISOR,
};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, AudioTraceTiming, Huc6280AudioTrace,
    Huc6280ChannelResetState, Huc6280ResetState, Huc6280TraceChip, Huc6280TraceRevision,
    Huc6280TraceWrite,
};
use zeff_emu_common::time::ClockRate;

impl PceMachine {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.devices().cdrom2().is_none() && self.devices().arcade_card().is_none(),
            "PC Engine audio tracing requires HuCard media without CD hardware"
        );
        let recorder = self.audio_trace.prepare_with_clock(
            max_events,
            ClockRate::from_ratio(
                PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR,
                PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR,
            ),
            reset_chip(self.devices().psg().revision()),
            AudioTraceTiming::MemoryWriteCompletion,
        )?;
        self.reset();
        self.audio_trace = recorder;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<Huc6280AudioTrace> {
        if self.faulted {
            self.audio_trace
                .invalidate(AudioTraceInvalidation::ExecutionFault);
        }
        self.audio_trace.finish(self.master_ticks)
    }

    pub(super) fn audio_trace_source(&self, pc: u16) -> AudioTraceSource {
        let physical = self.cpu.cpu().logical_to_physical(pc);
        if let Some(offset) = self.bus.hucard_rom_offset(physical) {
            return AudioTraceSource::CartridgeRom {
                offset: u64::from(offset),
                bit_reversed: false,
            };
        }
        match self.bus.decode_physical_region(physical) {
            PhysicalRegion::WorkRam(offset) => AudioTraceSource::WorkRam {
                offset: u32::from(offset),
            },
            PhysicalRegion::HuCard(offset)
                if self.bus.hucard_board() == PceHuCardBoard::Populous
                    && (0x08_0000..=0x08_7FFF).contains(&offset) =>
            {
                AudioTraceSource::CartridgeRam {
                    offset: offset - 0x08_0000,
                }
            }
            PhysicalRegion::HuCard(offset)
                if self.bus.hucard_board() == PceHuCardBoard::SystemCardV3
                    && (crate::hardware::cartridge::SUPER_SYSTEM_CARD_RAM_START
                        ..=crate::hardware::cartridge::SUPER_SYSTEM_CARD_RAM_END)
                        .contains(&offset) =>
            {
                AudioTraceSource::CartridgeRam {
                    offset: offset - crate::hardware::cartridge::SUPER_SYSTEM_CARD_RAM_START,
                }
            }
            PhysicalRegion::HuCard(_) | PhysicalRegion::Unmapped => AudioTraceSource::Unmapped,
            _ => AudioTraceSource::Unknown,
        }
    }
}

pub(super) struct TimedAudioTrace<'a> {
    pub(super) recorder: &'a mut Huc6280AudioTraceRecorder,
    pub(super) pc: u16,
    pub(super) instruction_source: AudioTraceSource,
    pub(super) start_cycle: u64,
}

impl TimedAudioTrace<'_> {
    pub(super) fn record_write(&mut self, physical_address: u32, value: u8, elapsed: u64) {
        let PhysicalRegion::Psg(port) = decode_physical_region(physical_address) else {
            return;
        };
        let Some(cycle) = self.start_cycle.checked_add(elapsed) else {
            self.recorder
                .invalidate(AudioTraceInvalidation::ClockOverflow);
            return;
        };
        self.recorder.record(AudioTraceEvent {
            cycle,
            pc: u32::from(self.pc),
            instruction_source: self.instruction_source,
            write: Huc6280TraceWrite {
                physical_address: physical_address & PHYSICAL_ADDRESS_MASK,
                register: port.offset(),
                value,
            },
        });
    }
}

fn reset_chip(revision: PsgRevision) -> Huc6280TraceChip {
    Huc6280TraceChip {
        clock_hz_numerator: PSG_CLOCK_NUMERATOR,
        clock_hz_denominator: PSG_CLOCK_DENOMINATOR as u32,
        master_clock_divisor: PSG_MASTER_CLOCK_DIVISOR as u8,
        internal_master_clock_divisor: PSG_INTERNAL_MASTER_CLOCK_DIVISOR as u8,
        revision: match revision {
            PsgRevision::HuC6280 => Huc6280TraceRevision::HuC6280,
            PsgRevision::HuC6280A => Huc6280TraceRevision::HuC6280A,
        },
        reset: Huc6280ResetState {
            channels: [Huc6280ChannelResetState {
                frequency: 0,
                control: 0,
                balance: 0,
                waveform: [0; 32],
                wave_index: 0,
                dda_hold: 0,
                noise_control: 0,
                wave_counter: 4096,
                noise_counter: 0,
                noise_seed: 1,
                effective_left_attenuation: 31,
                effective_right_attenuation: 31,
            }; 6],
            selected_channel: 0,
            main_amplitude: 0,
            lfo_frequency: 0,
            lfo_control: 0,
            lfo_counter: 0,
            lfo_phase_valid: false,
            gain_scan_clock: 0,
            gain_scan_active: false,
            gain_scan_queued: false,
            attenuation_latch: 31,
            master_tick_remainder: 0,
        },
    }
}

#[cfg(test)]
mod tests;
