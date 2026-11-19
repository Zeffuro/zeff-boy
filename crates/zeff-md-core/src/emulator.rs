use crate::cartridge::{Cartridge, LoadError};
use crate::constants::{MAIN_CPU_DIVISOR, PRIME_CLOCKS, STOP_POLL_TICKS, WORK_RAM_SIZE};
use crate::hardware::bus::scheduled::{CompactBus, ScheduledBus, expand_fault};
use crate::hardware::bus::{Bus, BusError, Prefetch};
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
    Input(crate::InputFault),
    PreciseTiming(String),
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
    call_stack: call_stack::Tracker,
}

mod call_stack;
pub use call_stack::{CallStackFrame, CallStackStatus};
mod owned;
pub use owned::TimingModel;
mod cpu_operation;
use crate::sound_trace::{NoSoundCapture, SoundCapture};
use cpu_operation::CpuOperation;
mod memory_observer;
use memory_observer::{Direct, Observer};
pub use memory_observer::{InstructionObserver, MemoryAccess};
mod audio;
mod debug_memory;
mod debug_ports;
pub use debug_ports::{DebugPortError, DebugPortReport, DebugPortWrite, DebugSavePorts};
mod guest_call;
pub use debug_memory::{DEBUG_MEMORY_RANGES, DEBUG_RAM_RANGE};
pub use guest_call::{DebugCallContext, GuestCallError};
pub mod cheats;
mod debugger_checkpoint;
#[cfg(test)]
mod extended_ssf_tests;
mod frontend_state;
#[cfg(test)]
mod linear_rom_tests;
#[cfg(test)]
mod protection_tests;
#[cfg(test)]
mod register_tests;
#[cfg(test)]
mod sf001_tests;
#[cfg(test)]
pub(crate) mod sf002_tests;
#[cfg(test)]
mod sf004_tests;
#[cfg(test)]
mod ssf2_tests;
pub use frontend_state::MAX_FRONTEND_STATE_BYTES;
mod audio_trace;
#[cfg(test)]
mod closed_sound_tests;
#[cfg(test)]
mod cpu_continuation_tests;
mod field;
mod input;
pub mod rollback;
pub use input::InputMut;
mod restart;
mod save_ram;
mod sound;
mod sound_trace;
mod svp;
mod tmss;
pub use sound::{WarmResetError, WarmResetFailure, WarmResetRejection, WarmResetReport};
#[cfg(test)]
mod sound_irq_tests;
mod state;
mod video;
pub use video::VdpCaptureError;

impl Emulator {
    pub fn supports_cartridge_size(rom: &[u8]) -> bool {
        rom.len() <= crate::constants::MAX_CARTRIDGE_SIZE
            || crate::cartridge::ssf2::Ssf2::detect(rom)
            || crate::cartridge::extended_ssf::ExtendedSsf::detect(rom)
    }

    pub fn new(rom: Vec<u8>, config: LoadConfig) -> Result<Self, LoadError> {
        Self::new_with_cartridge(Cartridge::new(rom)?, config)
    }

    pub(crate) fn new_with_cartridge(
        cartridge: Cartridge,
        config: LoadConfig,
    ) -> Result<Self, LoadError> {
        let mut machine = Self {
            cpu: PrefetchCpu::default(),
            bus: Bus::new(cartridge, config.region),
            sound_cpu: zeff_z80::Cpu::new(),
            main_stopped: false,
            concurrent_sound: config.concurrent_sound,
            sound_schedule: SoundSchedule::default(),
            call_stack: call_stack::Tracker::default(),
        };
        let pc = machine.bus.initial_registers().pc;
        machine
            .restart_cpu()
            .map_err(|_| LoadError::ResetProgramCounter(pc))?;
        Ok(machine)
    }

    pub fn step_instruction(&mut self) -> Result<MachineStep, MachineFault> {
        self.step_instruction_with(&mut Direct, &mut NoSoundCapture)
    }

    /// Reports successful operand/stack transfers without interrupting the CPU boundary.
    pub fn step_instruction_observed(
        &mut self,
        observer: &mut impl FnMut(MemoryAccess),
    ) -> Result<MachineStep, MachineFault> {
        self.step_instruction_with(&mut Observer(observer), &mut NoSoundCapture)
    }

    pub fn step_instruction_traced(
        &mut self,
        observer: &mut impl InstructionObserver,
    ) -> Result<MachineStep, MachineFault> {
        self.step_instruction_with(&mut Observer(observer), &mut NoSoundCapture)
    }

    fn step_instruction_with<C: SoundCapture>(
        &mut self,
        observer: &mut impl CpuOperation<Output = TimedOutcome>,
        capture: &mut C,
    ) -> Result<MachineStep, MachineFault> {
        if let Some(fault) = self.input_fault() {
            return Err(MachineFault::Input(fault));
        }
        if self.concurrent_sound && self.bus.owned.is_none() {
            self.bus.audio.begin_batch();
        }
        let result = self.step_traced_boundary_with(observer, capture);
        self.finish_execution(result.as_ref().err());
        result
    }

    pub fn step_field(&mut self) -> Result<(), MachineFault> {
        let mut execution = field::FieldExecution::begin(self)?;
        loop {
            match execution.resume(self, std::num::NonZeroUsize::MAX)? {
                Some(pending) => execution = pending,
                None => return Ok(()),
            }
        }
    }

    fn step_traced_boundary(&mut self) -> Result<MachineStep, MachineFault> {
        self.step_traced_boundary_with(&mut Direct, &mut NoSoundCapture)
    }

    fn step_traced_boundary_with<C: SoundCapture>(
        &mut self,
        observer: &mut impl CpuOperation<Output = TimedOutcome>,
        capture: &mut C,
    ) -> Result<MachineStep, MachineFault> {
        if self.bus.owned.is_some() {
            return self.step_owned_boundary(observer, &mut capture.physical());
        }
        self.begin_main_trace();
        #[cfg(feature = "first-failure-diagnostic")]
        if crate::diagnostic::enabled() {
            crate::diagnostic::instruction_entry(self.diagnostic_snapshot());
        }
        #[cfg(feature = "first-failure-diagnostic")]
        {
            let observing = crate::diagnostic::field::enabled();
            if observing {
                crate::diagnostic::field::boundary_entry(
                    self.diagnostic_snapshot(),
                    self.pacing_field_count(),
                );
            }
            let result = self.step_main_boundary_with(observer, capture);
            if observing {
                crate::diagnostic::field::boundary_exit(
                    self.diagnostic_snapshot(),
                    self.pacing_field_count(),
                    &result,
                );
            }
            result
        }
        #[cfg(not(feature = "first-failure-diagnostic"))]
        self.step_main_boundary_with(observer, capture)
    }

    fn finish_execution(&mut self, fault: Option<&MachineFault>) {
        if self.concurrent_sound && self.bus.owned.is_none() {
            self.bus.audio.finish_batch();
        }
        self.bus.audio.trace_origin = None;
        if let Some(fault) = fault {
            self.invalidate_trace_fault(fault);
        }
    }

    #[cfg(test)]
    fn step_main_boundary(&mut self) -> Result<MachineStep, MachineFault> {
        self.step_main_boundary_with(&mut Direct, &mut NoSoundCapture)
    }

    fn step_main_boundary_with<C: SoundCapture>(
        &mut self,
        observer: &mut impl CpuOperation<Output = TimedOutcome>,
        capture: &mut C,
    ) -> Result<MachineStep, MachineFault> {
        let step = self.step_main_operation_with(observer, capture)?;
        Ok(MachineStep {
            cpu: step.output,
            elapsed: step.elapsed,
        })
    }

    pub fn main_cpu_stopped(&self) -> bool {
        self.cpu.stopped()
    }
    pub fn registers(&self) -> Registers {
        self.cpu.registers
    }
    #[cfg(feature = "first-failure-diagnostic")]
    pub(crate) fn diagnostic_snapshot(&self) -> crate::diagnostic::MachineSnapshot {
        crate::diagnostic::MachineSnapshot {
            registers: self.cpu.registers,
            queued_words: self.cpu.queued_words(),
            cpu_clock: self.cpu.clock().0,
            context: crate::diagnostic::context::scheduled_context(
                &self.bus,
                &self.sound_cpu,
                &self.sound_schedule,
                None,
            ),
            concurrent_sound: self.concurrent_sound,
            audio_trace_enabled: self.bus.audio.trace.is_enabled(),
            region: self.bus.region,
        }
    }
    #[cfg(feature = "first-failure-diagnostic")]
    pub fn diagnostic_pending_sound(&self) -> Option<crate::diagnostic::PendingSoundObservation> {
        let pending = self.sound_schedule.pending.as_ref()?;
        Some(crate::diagnostic::PendingSoundObservation {
            committed_pc: self.sound_cpu.regs().pc,
            committed_cycles: self.sound_cpu.cycles(),
            before: self.sound_schedule.state,
            after: pending.state,
            instruction_pc: pending.instruction.pc,
            instruction_opcode: pending.instruction.opcode,
            segment_cycles: pending.instruction.cycles,
            start: pending.start,
            deadline: pending.deadline,
            unclocked_bytes: std::array::from_fn(|offset| {
                self.bus
                    .sound
                    .read(pending.instruction.pc.wrapping_add(offset as u16))
            }),
            epoch_present: self.sound_schedule.epoch.is_some(),
        })
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
    pub fn work_ram(&self) -> &[u8] {
        &self.bus.ram
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
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(self.bus.now, self.bus.rate())
    }
}

impl Reset for Emulator {
    fn reset(&mut self) {
        if self.bus.owned.is_some() {
            self.power_on_owned();
            return;
        }
        self.invalidate_audio_trace(zeff_emu_common::audio_trace::AudioTraceInvalidation::Reset);
        if let Some(svp) = &mut self.bus.cartridge.svp {
            svp.reset_synthetic();
        }
        if let Some(board) = &mut self.bus.cartridge.registers {
            board.reset_bank();
        }
        if let Some(save) = &mut self.bus.cartridge.save {
            save.reset_mapping(self.bus.now.get());
        }
        if let Some(board) = &mut self.bus.cartridge.sf002 {
            *board = crate::cartridge::sf002::Sf002::new();
        }
        if let Some(board) = &mut self.bus.cartridge.sf004 {
            board.power_on();
        }
        if let Some(board) = &mut self.bus.cartridge.realtec {
            board.reset();
        }
        if let Some(board) = &mut self.bus.cartridge.extended_ssf {
            board.power_on();
        }
        if let Some(board) = &mut self.bus.cartridge.ssf2 {
            board.power_on();
        }
        if let Some(board) = &mut self.bus.cartridge.sf001 {
            board.power_on();
        }
        if let Some(save) = &mut self.bus.cartridge.lock_on_save {
            save.enabled = false;
        }
        if let Some(board) = &mut self.bus.cartridge.lock_on {
            board.write_control(0);
        }
        if let Some(board) = &mut self.bus.cartridge.parallel_adapter {
            board.power_on();
        }
        if let Some(board) = &mut self.bus.cartridge.download {
            board.power_on(&self.bus.cartridge.bytes);
        }
        self.bus.ram.fill(0);
        if let Some(tmss) = &mut self.bus.tmss {
            tmss.power_on();
        }
        self.bus.now = MasterTicks::ZERO;
        self.bus.irq_level = 0;
        self.bus.vdp.power_on();
        self.bus.video.reset();
        self.bus.audio.reset();
        self.bus.reset_pulses = 0;
        self.bus.reset_output = false;
        self.bus.rmw = None;
        self.bus.sound = Default::default();
        self.bus.input.power_on();
        self.sound_cpu.reset();
        self.sound_schedule = Default::default();
        self.restart_cpu().expect("validated cartridge restart");
    }
}

#[cfg(test)]
#[path = "emulator/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "emulator/save_latch_tests.rs"]
pub(crate) mod save_latch_tests;

#[cfg(test)]
#[path = "emulator/download_tests.rs"]
pub(crate) mod download_tests;

#[cfg(test)]
mod batch_tests;

#[cfg(test)]
mod field_tests;

#[cfg(test)]
mod diagnostic_contract_tests;

#[cfg(test)]
mod parallel_adapter_tests;
