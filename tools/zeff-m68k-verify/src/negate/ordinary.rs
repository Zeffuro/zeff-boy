use super::{Device, Plan};
use crate::reference::Case;
use zeff_m68k::bus::BusAccess;
use zeff_m68k::timing::physical::{PhysicalEntryState, PhysicalInspection};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};

impl ClockedBus for Device<'_> {
    type Error = String;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, String> {
        CycleResult {
            result: self.transfer(access, start, None, false),
            wait_clocks: 0,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), String> {
        CycleResult {
            result: self.transfer(access, start, Some(value), false).map(|_| ()),
            wait_clocks: 0,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.error.is_none()
            && let Err(error) = Device::idle(self, start, clocks)
        {
            self.error = Some(error);
        }
    }
}

pub fn verify(case: &Case) -> Result<(), String> {
    let plan = Plan::new(case)?;
    if plan.odd {
        return Err("ordinary completion requires aligned source".into());
    }
    let mut cpu = PrefetchCpu::from_prefetched(plan.initial, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial PC")?;
    let mut device = Device::new(&plan, &case.initial.ram)?;
    let result = cpu
        .step_instruction(&mut device)
        .map_err(|error| format!("ordinary step: {error:?}"))?;
    if result.opcode != case.initial.prefetch[0]
        || result.exception.is_some()
        || result.clocks != plan.total
        || cpu.registers != plan.final_registers
        || cpu.queued_words() != Some(plan.words)
        || cpu.clock().0 != plan.total
        || device.elapsed != plan.total
        || device.memory != plan.memory
        || device.cursor != plan.steps.len()
        || device.idle_remaining != 0
        || device.error.is_some()
        || cpu.trace_pending()
        || cpu.stopped()
        || cpu.interrupt_input().level() != 0
        || cpu.physical_entry_state() != PhysicalEntryState::Idle
        || cpu.physical_inspection() != PhysicalInspection::DiagnosticCompleteValues
    {
        return Err("ordinary full state/RAM/queue/ordered callback consumption differs".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
