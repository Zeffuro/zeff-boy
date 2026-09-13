use super::Bus;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, GameBoyDividerResetCause,
    GameBoyResetKind, GameBoyResetState, GameBoyTraceChip, GameBoyTraceModel, GameBoyTraceOrigin,
    GameBoyTraceWrite,
};

impl Bus {
    pub(crate) fn audio_trace_chip(&self) -> GameBoyTraceChip {
        GameBoyTraceChip {
            clock_hz: GB_T_CYCLES_PER_SECOND as u32,
            model: if self.is_cgb_hardware() {
                GameBoyTraceModel::Cgb
            } else {
                GameBoyTraceModel::Dmg
            },
            dmg_compatibility: self.cgb_dmg_compat,
            reset: GameBoyResetState {
                kind: if self.boot_rom_enabled {
                    GameBoyResetKind::PowerOn
                } else {
                    GameBoyResetKind::PostBoot
                },
                registers: self.io.apu.regs_snapshot(),
                wave_ram: self.io.apu.wave_ram_snapshot(),
                nr52: self.io.apu.nr52_raw(),
                divider_counter: self.io.timer.divider_counter(),
                double_speed: self.hardware_mode == HardwareMode::CGBDouble,
            },
        }
    }

    pub(crate) fn audio_trace_apu_enabled(&self) -> bool {
        self.io.apu.apu_enabled
    }

    pub(crate) fn begin_audio_trace_instruction(&mut self, pc: u16, cycle: u64) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        self.check_audio_trace_clock(cycle);
        let boot_mapped = self.boot_rom_enabled
            && (pc <= 0xff || (self.is_cgb_hardware() && (0x200..=0x8ff).contains(&pc)));
        let source = if self.oam_dma_blocks_cpu_access(pc) {
            AudioTraceSource::Unknown
        } else if boot_mapped {
            AudioTraceSource::BootRom {
                offset: u64::from(pc),
            }
        } else if let Some(offset) = self.cartridge.rom_offset(pc) {
            AudioTraceSource::CartridgeRom {
                offset: offset as u64,
                bit_reversed: false,
            }
        } else if (WRAM_0_START..=ECHO_RAM_END).contains(&pc) {
            let address = if pc >= ECHO_RAM_START {
                pc - ECHO_RAM_OFFSET
            } else {
                pc
            };
            let offset = if address < WRAM_N_START {
                usize::from(address - WRAM_0_START)
            } else {
                self.active_wram_bank() * WRAM_SIZE + usize::from(address - WRAM_N_START)
            };
            AudioTraceSource::WorkRam {
                offset: offset as u32,
            }
        } else if (HRAM_START..=HRAM_END).contains(&pc) {
            // HRAM follows the physical 32 KiB WRAM in this trace's work-RAM space.
            AudioTraceSource::WorkRam {
                offset: 0x8000 + u32::from(pc - HRAM_START),
            }
        } else {
            AudioTraceSource::Unknown
        };
        self.audio_trace_context = (u32::from(pc), source);
        self.audio_trace_origin = GameBoyTraceOrigin::Cpu;
    }

    pub(crate) fn end_audio_trace_instruction(&mut self, cycle: u64) {
        if self.audio_trace.is_enabled() {
            self.check_audio_trace_clock(cycle);
            self.audio_trace_context = (0, AudioTraceSource::Unknown);
            self.audio_trace_origin = GameBoyTraceOrigin::Cpu;
        }
    }

    pub(in crate::hardware) fn begin_audio_trace_interrupt(&mut self) {
        if self.audio_trace.is_enabled() {
            self.audio_trace_context = (0, AudioTraceSource::Unknown);
            self.audio_trace_origin = GameBoyTraceOrigin::CpuInterrupt;
        }
    }

    pub(super) fn advance_audio_trace_clock(&mut self, cycles: u64) {
        if self.audio_trace.is_enabled() {
            match self.audio_trace_cycle.checked_add(cycles) {
                Some(cycle) => self.audio_trace_cycle = cycle,
                None => self
                    .audio_trace
                    .invalidate(AudioTraceInvalidation::ClockOverflow),
            }
        }
    }

    fn check_audio_trace_clock(&mut self, cycle: u64) {
        if self.audio_trace_cycle != cycle {
            self.audio_trace
                .invalidate(AudioTraceInvalidation::NonMonotonicCycles);
        }
    }

    pub(crate) fn trace_audio_stop(&mut self, entered: bool) {
        if self.audio_trace.is_enabled() && self.audio_trace_stopped != entered {
            self.record_audio_write(GameBoyTraceWrite::Stop { entered });
            self.audio_trace_stopped = entered;
        }
    }

    pub(super) fn trace_audio_speed_switch(&mut self) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(GameBoyTraceWrite::SpeedSwitch {
                double_speed: self.hardware_mode == HardwareMode::CGBDouble,
            });
        }
    }

    pub(super) fn trace_audio_speed_switch_delay(&mut self, cycles: u64) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(GameBoyTraceWrite::SpeedSwitchDelay { cycles });
        }
    }

    pub(super) fn trace_audio_divider_reset(&mut self, cause: GameBoyDividerResetCause) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(GameBoyTraceWrite::DividerReset {
                cause,
                divider_counter: self.io.timer.divider_counter(),
                apu_bit: self.io.timer.div_apu_bit(),
            });
        }
    }

    pub(super) fn trace_audio_sequencer(&mut self, primary: u8, secondary: u8) {
        if self.audio_trace.is_enabled() && (primary != 0 || secondary != 0) {
            // Timer events are applied before the native batch advances APU oscillators.
            self.audio_trace.record(AudioTraceEvent {
                cycle: self.audio_trace_cycle,
                pc: 0,
                instruction_source: AudioTraceSource::Unknown,
                write: GameBoyTraceWrite::SequencerClock { primary, secondary },
            });
        }
    }

    pub(super) fn trace_audio_register(&mut self, address: u16, value: u8) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(GameBoyTraceWrite::Register {
                address,
                value,
                origin: self.audio_trace_origin,
            });
        }
    }

    pub(super) fn trace_audio_wave_ram(&mut self, address: u16, value: u8) {
        if self.audio_trace.is_enabled() {
            self.record_audio_write(GameBoyTraceWrite::WaveRam {
                address,
                value,
                applied_index: self
                    .io
                    .apu
                    .wave_ram_cpu_access_index(address)
                    .map(|index| index as u8),
                origin: self.audio_trace_origin,
            });
        }
    }

    fn record_audio_write(&mut self, write: GameBoyTraceWrite) {
        let (pc, instruction_source) = self.audio_trace_context;
        self.audio_trace.record(AudioTraceEvent {
            cycle: self.audio_trace_cycle,
            pc,
            instruction_source,
            write,
        });
    }
}
