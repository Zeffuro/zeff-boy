use super::cpu_operation::CpuOperation;
use super::*;
use crate::constants::CARTRIDGE_END;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;
use zeff_m68k::bus::BUS_ADDRESS_MASK;
pub use zeff_m68k::timing::DebugCallContext;
use zeff_m68k::timing::{ClockedBus, DebugCallRejection};

#[derive(Debug, PartialEq, Eq)]
pub enum GuestCallError {
    Rejected(&'static str),
    Cpu(DebugCallRejection),
    Execution(MachineFault),
}

struct Entry(u32);

impl CpuOperation for Entry {
    type Output = DebugCallContext;
    const SAMPLE_IRQ_INPUT: bool = false;

    fn step<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<Self::Output, TimingFault<B::Error>> {
        cpu.begin_debug_call(bus, self.0)
    }
}

impl Emulator {
    pub fn check_debug_call(&self, target: u32) -> Result<(), GuestCallError> {
        self.cpu
            .check_debug_call(target)
            .map_err(GuestCallError::Cpu)?;
        if target > BUS_ADDRESS_MASK {
            return Err(GuestCallError::Rejected(
                "target exceeds the 24-bit address space",
            ));
        }
        if self.bus.interrupt_level() == 7 {
            return Err(GuestCallError::Cpu(DebugCallRejection::Level7Interrupt));
        }
        #[cfg(feature = "first-failure-diagnostic")]
        if crate::diagnostic::enabled() || crate::diagnostic::field::enabled() {
            return Err(GuestCallError::Rejected(
                "instruction diagnostic capture is active",
            ));
        }
        if let Some(fault) = self.input_fault() {
            return Err(GuestCallError::Execution(MachineFault::Input(fault)));
        }
        if self.bus.clock_fault {
            return Err(GuestCallError::Execution(TimingFault::ClockOverflow.into()));
        }
        if let Some(fault) = self.sound_schedule.fault {
            return Err(GuestCallError::Execution(MachineFault::Sound(fault)));
        }
        if self.bus.audio.batch_active() {
            return Err(GuestCallError::Rejected("audio batch is active"));
        }
        let stack = self.cpu.registers.stack_pointer().wrapping_sub(4);
        if !(0..4).all(|offset| {
            let address = stack.wrapping_add(offset) & BUS_ADDRESS_MASK;
            (DEBUG_RAM_RANGE.0..=DEBUG_RAM_RANGE.1).contains(&address)
        }) {
            return Err(GuestCallError::Rejected(
                "return stack must use canonical work RAM",
            ));
        }
        if !self.debug_call_fetch_span(target) {
            return Err(GuestCallError::Rejected(
                "target prefetch is outside qualified executable memory",
            ));
        }
        if !self.debug_call_fetch_span(self.cpu.registers.pc) {
            return Err(GuestCallError::Rejected(
                "return prefetch is outside qualified executable memory",
            ));
        }
        self.encode_state().map(|_| ()).map_err(|_| {
            GuestCallError::Rejected("current machine state is not qualified for a guest call")
        })
    }

    pub fn begin_debug_call(&mut self, target: u32) -> Result<DebugCallContext, GuestCallError> {
        self.begin_debug_call_captured(target, &mut NoSoundCapture)
    }

    pub(super) fn begin_debug_call_captured<C: SoundCapture>(
        &mut self,
        target: u32,
        capture: &mut C,
    ) -> Result<DebugCallContext, GuestCallError> {
        self.check_debug_call(target)?;
        self.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        self.bus.audio.trace_origin = None;
        if self.concurrent_sound && self.bus.owned.is_none() {
            self.bus.audio.begin_batch();
        }
        let result = if self.bus.owned.is_some() {
            self.step_owned_operation_capturing(&mut Entry(target), &mut capture.physical())
        } else {
            self.step_main_operation_with(&mut Entry(target), capture)
        };
        self.finish_execution(result.as_ref().err());
        result.map(|step| step.output).map_err(call_fault)
    }

    pub fn debug_call_returned(&self, context: &DebugCallContext) -> bool {
        self.bus.interrupt_level() != 7 && self.cpu.debug_call_returned(context)
    }

    pub fn finish_debug_call(&mut self, context: DebugCallContext) -> Result<(), GuestCallError> {
        if self.bus.interrupt_level() == 7 {
            return Err(GuestCallError::Cpu(DebugCallRejection::Level7Interrupt));
        }
        self.cpu
            .finish_debug_call(context)
            .map_err(GuestCallError::Cpu)
    }

    pub fn rom_offset_for_cpu_address(&self, address: u32) -> Option<usize> {
        let address = address & BUS_ADDRESS_MASK;
        let cartridge = &self.bus.cartridge;
        if cartridge.svp.is_some()
            || self
                .bus
                .tmss
                .as_ref()
                .is_some_and(|tmss| tmss.boot_mapped())
            || !cartridge.supports_rom_overlay()
            || address > CARTRIDGE_END
            || cartridge.realtec.is_some() && (0x100..=0x101).contains(&address)
            || cartridge.save.as_ref().is_some_and(|save| {
                cartridge.save_latch.is_none() && save.contains(address) && !save.rom_visible()
            })
        {
            return None;
        }
        cartridge.rom_source_offset(address)
    }

    fn debug_call_fetch_span(&self, address: u32) -> bool {
        (0..4).all(|offset| {
            let address = address.wrapping_add(offset) & BUS_ADDRESS_MASK;
            (DEBUG_RAM_RANGE.0..=DEBUG_RAM_RANGE.1).contains(&address)
                || self.rom_offset_for_cpu_address(address).is_some()
        }) || self.bus.tmss.is_none()
            && self.bus.owned.is_none()
            && self.bus.cartridge.debug_download_fetch_span(address)
    }
}

fn call_fault(fault: MachineFault) -> GuestCallError {
    match fault {
        MachineFault::Main(TimingFault::DebugCallRejected(reason)) => GuestCallError::Cpu(reason),
        fault => GuestCallError::Execution(fault),
    }
}

#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod download_tests;
#[cfg(test)]
mod tests;
