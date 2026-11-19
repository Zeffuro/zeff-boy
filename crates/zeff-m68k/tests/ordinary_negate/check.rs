use super::*;
use fixture::Outcome;

fn retries(cpu: &mut PrefetchCpu, device: &mut device::Device) -> bool {
    let saved = cpu.clone();
    let saved_device = device.clone();
    let mut copied = saved.clone();
    let mut copied_device = saved_device.clone();
    let mut good = copied.step_instruction(&mut copied_device) == Err(TimingFault::Frozen);
    good &= copied == saved && copied_device == saved_device;
    good &= cpu.step_instruction(device) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *device == saved_device;
    good &= cpu.step_boundary(device) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *device == saved_device;
    good &= cpu.prime(device) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *device == saved_device;
    good &= cpu.service_pending_trace(device) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *device == saved_device;
    good &= cpu.step(device) == Err(TimingFault::Frozen);
    good &= *cpu == saved && *device == saved_device;
    good
}

pub(super) fn run(c: fixture::Case) -> (bool, bool) {
    let mut cpu = PrefetchCpu::from_prefetched(c.initial, c.queue, CpuClocks(c.start)).unwrap();
    let mut device = device::Device::new(&c);
    let e = &c.expected;
    let expected = match e.outcome {
        Outcome::Complete => Ok(TimedStep {
            opcode: c.opcode,
            clocks: e.duration,
            exception: None,
        }),
        Outcome::Host(access, direction, source) => Err(TimingFault::Bus {
            access,
            direction,
            source,
        }),
        Outcome::Overflow => Err(TimingFault::ClockOverflow),
    };
    let actual = cpu.step_instruction(&mut device);
    let mut without_sr = cpu.registers;
    without_sr.set_status_register(e.r.status_register());
    let expected_device = device::Device {
        clock: e.clock,
        words: e.ram.iter().copied().collect(),
        events: c.events.clone(),
        waits: c.waits.clone(),
        fail: c.fail,
        failed_write_deposits: c.failed_write_deposits,
        count: c.events.len(),
        valid: true,
    };
    let mut other = fixture::consistent(&c)
        && actual == expected
        && without_sr == e.r
        && cpu.queued_words() == Some(e.queue)
        && cpu.clock().0 == e.clock
        && device == expected_device
        && cpu.trace_pending() == e.trace
        && cpu.stopped() == e.stopped
        && *cpu.interrupt_input() == Default::default()
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && private::matches(&cpu, e);
    if e.frozen {
        other &= retries(&mut cpu, &mut device);
    }
    let sr_equal = cpu.registers.status_register() == e.r.status_register();
    let good = other && sr_equal;
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary NEG/NEGX {} SR-only={} actual={actual:?} expected={expected:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} device={device:?}/{expected_device:?}",
            c.label,
            other && !sr_equal,
            cpu.registers,
            e.r,
            cpu.queued_words(),
            e.queue,
            cpu.clock().0,
            e.clock
        );
    }
    let _ = c.label;
    (good, other && !sr_equal)
}
