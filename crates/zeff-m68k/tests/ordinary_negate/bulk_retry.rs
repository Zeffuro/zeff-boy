use super::*;

pub(crate) fn frozen<B>(cpu: &mut PrefetchCpu, bus: &mut B) -> bool
where
    B: ClockedBus + Clone + PartialEq,
    B::Error: PartialEq,
{
    let saved = cpu.clone();
    let device = bus.clone();
    let mut copied = saved.clone();
    let mut copied_device = device.clone();
    let mut good = copied.step_instruction(&mut copied_device) == Err(TimingFault::Frozen);
    good &= copied == saved && copied_device == device;
    good &= cpu.step_instruction(bus) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *bus == device;
    good &= cpu.step_boundary(bus) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *bus == device;
    good &= cpu.prime(bus) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *bus == device;
    good &= cpu.service_pending_trace(bus) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *bus == device;
    good &= cpu.step(bus) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *bus == device;
    good
}
