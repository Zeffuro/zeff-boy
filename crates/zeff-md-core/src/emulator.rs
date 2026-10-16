use crate::cartridge::{Cartridge, LoadError};
use crate::constants::{MAIN_CPU_DIVISOR, PRIME_CLOCKS, STOP_POLL_TICKS, WORK_RAM_SIZE};
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::bus::{Bus, BusError};
use crate::hardware::sound::{SoundFault, schedule::SoundSchedule};
use crate::hardware::vdp::{Vdp, VdpRegisterWrite, VdpScheduleError};
use zeff_emu_common::time::{MachineTiming, MasterTicks, Reset, TimingSnapshot};
use zeff_m68k::Registers;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu, TimedOutcome, TimingFault};

pub use crate::hardware::region::Region;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadConfig {
    pub region: Region,
    pub concurrent_sound: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineFault {
    Main(TimingFault<BusError>),
    Sound(SoundFault),
}
impl From<TimingFault<BusError>> for MachineFault {
    fn from(fault: TimingFault<BusError>) -> Self {
        Self::Main(fault)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineStep {
    pub cpu: TimedOutcome,
    pub elapsed: MasterTicks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emulator {
    cpu: PrefetchCpu,
    bus: Bus,
    sound_cpu: zeff_z80::Cpu,
    main_stopped: bool,
    concurrent_sound: bool,
    sound_schedule: SoundSchedule,
}

mod audio;
mod audio_trace;
#[cfg(test)]
mod cpu_continuation_tests;
mod input;
mod save_ram;
mod sound;
#[cfg(test)]
mod sound_irq_tests;
mod state;
mod video;
pub use video::VdpCaptureError;

impl Emulator {
    pub fn new(rom: Vec<u8>, config: LoadConfig) -> Result<Self, LoadError> {
        let cartridge = Cartridge::new(rom)?;
        let mut machine = Self {
            cpu: PrefetchCpu::default(),
            bus: Bus::new(cartridge, config.region),
            sound_cpu: zeff_z80::Cpu::new(),
            main_stopped: false,
            concurrent_sound: config.concurrent_sound,
            sound_schedule: SoundSchedule::default(),
        };
        machine.reset_cpu().expect("validated cartridge startup");
        Ok(machine)
    }

    /// Publishes sound before main transfers, reserving the bus for banked accesses.
    pub fn step_instruction(&mut self) -> Result<MachineStep, MachineFault> {
        self.begin_main_trace();
        let result = self.step_main_boundary();
        self.bus.audio.trace_origin = None;
        if let Err(fault) = &result {
            self.invalidate_trace_fault(fault);
        }
        result
    }

    fn step_main_boundary(&mut self) -> Result<MachineStep, MachineFault> {
        if let Some(fault) = self.sound_schedule.fault {
            return Err(MachineFault::Sound(fault));
        }
        self.main_stopped = false;
        let before = self.bus.now;
        let sound_reset_before = self.bus.sound.reset_asserted;
        self.cpu
            .interrupt_input()
            .set_level(self.bus.interrupt_level());
        let result = if self.concurrent_sound {
            self.cpu.step_boundary(&mut ScheduledBus {
                bus: &mut self.bus,
                cpu: &mut self.sound_cpu,
                schedule: &mut self.sound_schedule,
            })
        } else {
            self.cpu.step_boundary(&mut self.bus)
        };
        if !self.concurrent_sound && !sound_reset_before && self.bus.sound.reset_asserted {
            self.sound_cpu.reset();
        }
        if !self.concurrent_sound {
            self.bus.sound.complete_boundary();
        }
        let cpu = result?;
        if self.bus.clock_fault {
            return Err(TimingFault::ClockOverflow.into());
        }
        if cpu == TimedOutcome::Stopped {
            let result = if self.concurrent_sound {
                ScheduledBus {
                    bus: &mut self.bus,
                    cpu: &mut self.sound_cpu,
                    schedule: &mut self.sound_schedule,
                }
                .advance_machine(STOP_POLL_TICKS)
            } else {
                self.bus.advance_machine(STOP_POLL_TICKS)
            };
            result.map_err(|_| TimingFault::ClockOverflow)?;
            self.bus.prefetch_upper = None;
        }
        self.main_stopped = cpu == TimedOutcome::Stopped;
        if let Some(fault) = self.sound_schedule.fault {
            return Err(MachineFault::Sound(fault));
        }
        Ok(MachineStep {
            cpu,
            elapsed: self.bus.now.checked_duration_since(before).unwrap(),
        })
    }

    pub fn registers(&self) -> Registers {
        self.cpu.registers
    }
    pub fn region(&self) -> Region {
        self.bus.region
    }
    pub fn cartridge_header(&self) -> Option<&crate::CartridgeHeader> {
        self.bus.cartridge.header.as_deref()
    }
    pub fn rom_bytes(&self) -> &[u8] {
        &self.bus.cartridge.bytes
    }
    pub fn cpu_clock(&self) -> CpuClocks {
        self.cpu.clock()
    }
    pub fn queued_words(&self) -> Option<[u16; 2]> {
        self.cpu.queued_words()
    }
    pub fn peek(&self, address: u32) -> Option<u8> {
        self.bus.peek(address)
    }
    pub fn interrupt_level(&self) -> u8 {
        self.bus.interrupt_level()
    }
    pub fn vdp(&self) -> &Vdp {
        &self.bus.vdp
    }
    pub fn queue_vdp_register(
        &mut self,
        index: u8,
        value: u8,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        let result = self.bus.vdp.queue_register(index, value, self.bus.region);
        if result.is_ok() {
            self.invalidate_audio_trace(
                zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
            );
        }
        result
    }
    pub fn reset_pulses(&self) -> u64 {
        self.bus.reset_pulses
    }

    /// Nonzero diagnostic override; zero restores VDP interrupt routing.
    pub fn set_interrupt_level(&mut self, level: u8) -> bool {
        if level > 7 {
            return false;
        }
        self.bus.irq_level = level;
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        true
    }

    /// Boundary restart preserves machine time, RAM and external interrupt inputs.
    pub fn reset_cpu(&mut self) -> Result<(), TimingFault<BusError>> {
        if self
            .bus
            .now
            .get()
            .checked_add(PRIME_CLOCKS * MAIN_CPU_DIVISOR)
            .is_none()
        {
            return Err(TimingFault::ClockOverflow);
        }
        // Priming may publish sound before a failed restart restores its checkpoint.
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        let mut cpu = PrefetchCpu::default();
        cpu.registers = self.bus.cartridge.initial;
        let clocks = (
            self.bus.now,
            self.bus.cpu_clock,
            self.bus.clock_fault,
            self.bus.prefetch_upper,
            self.bus.vdp.checkpoint(),
            self.bus.video.checkpoint(),
            self.bus.audio.checkpoint(),
        );
        let checkpoint = self.concurrent_sound.then(|| {
            let mut ram = [0; WORK_RAM_SIZE];
            ram.copy_from_slice(&self.bus.ram);
            (
                self.bus.sound.clone(),
                self.sound_cpu.clone(),
                self.sound_schedule.clone(),
                ram,
            )
        });
        self.bus.cpu_clock = CpuClocks(0);
        self.bus.clock_fault = false;
        let result = if self.concurrent_sound {
            cpu.prime(&mut ScheduledBus {
                bus: &mut self.bus,
                cpu: &mut self.sound_cpu,
                schedule: &mut self.sound_schedule,
            })
        } else {
            cpu.prime(&mut self.bus)
        };
        if let Err(fault) = result {
            let (now, clock, clock_fault, prefetch, vdp, video, audio) = clocks;
            self.bus.now = now;
            self.bus.cpu_clock = clock;
            self.bus.clock_fault = clock_fault;
            self.bus.prefetch_upper = prefetch;
            self.bus.vdp.restore(vdp);
            self.bus.video.restore(video);
            self.bus.audio.restore(audio);
            if let Some((sound, sound_cpu, schedule, ram)) = checkpoint {
                self.bus.sound = sound;
                self.sound_cpu = sound_cpu;
                self.sound_schedule = schedule;
                self.bus.ram.copy_from_slice(&ram);
            }
            return Err(fault);
        }
        self.cpu = cpu;
        self.main_stopped = false;
        Ok(())
    }
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(self.bus.now, self.bus.rate())
    }
}

impl Reset for Emulator {
    fn reset(&mut self) {
        self.invalidate_audio_trace(zeff_emu_common::audio_trace::AudioTraceInvalidation::Reset);
        if let Some(save) = &mut self.bus.cartridge.save {
            save.reset_mapping();
        }
        self.bus.ram.fill(0);
        self.bus.now = MasterTicks::ZERO;
        self.bus.irq_level = 0;
        self.bus.vdp.power_on();
        self.bus.video.reset();
        self.bus.audio.reset();
        self.bus.reset_pulses = 0;
        self.bus.reset_output = false;
        self.bus.sound = Default::default();
        self.bus.input.power_on();
        self.sound_cpu.reset();
        self.sound_schedule = Default::default();
        self.reset_cpu().expect("validated cartridge restart");
    }
}

#[cfg(test)]
#[path = "emulator/tests.rs"]
mod tests;
