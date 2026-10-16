use super::*;
use bulk_device::Device;
use bulk_model::Spec;
use bulk_oracle::Expected;
use bulk_plan::Plan;

#[derive(Default)]
pub(super) struct Count {
    pub count: u32,
    pub bad: u32,
}

impl Count {
    pub fn planned(&mut self, spec: Spec, device: Device, plan: Plan) {
        self.bad += u32::from(!run(spec, device, plan, self.bad < 3));
        self.count += 1;
    }
    pub fn result(self) -> (u32, u32) {
        (self.count, self.bad)
    }
}

pub(super) fn compare(
    cpu: &PrefetchCpu,
    device: &Device,
    expected: &Expected,
    result: &Result<TimedStep, TimingFault<usize>>,
) -> bool {
    result == &expected.result
        && cpu.registers == expected.r
        && cpu.queued_words() == Some(expected.queue)
        && cpu.clock().0 == expected.clock
        && device.words == expected.words
        && device.events == expected.events
        && device.count == expected.count
        && *device == expected.device
        && device.valid
        && cpu.trace_pending() == expected.trace
        && !cpu.stopped()
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues
}

pub(super) fn run(spec: Spec, mut device: Device, plan: Plan, diagnose: bool) -> bool {
    let expected = bulk_oracle::expect(spec, &device, &plan);
    if expected.result.is_ok() {
        assert_eq!(expected.queue, [plan.retained, plan.fetched]);
    }
    let mut cpu =
        PrefetchCpu::from_prefetched(spec.r, spec.queue(), CpuClocks(device.clock)).unwrap();
    let result = cpu.step_instruction(&mut device);
    let mut good = compare(&cpu, &device, &expected, &result);
    good &= *cpu.interrupt_input() == Default::default();
    good &= bulk_private::matches(&cpu, &expected);
    if !good && diagnose {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary NEG/NEGX bulk {spec:?} plan={plan:?} actual={result:?} expected={:?} registers={:?}/{:?} queue={:?}/{:?} clock={}/{} device={device:?}/{:?}",
            expected.result,
            cpu.registers,
            expected.r,
            cpu.queued_words(),
            expected.queue,
            cpu.clock().0,
            expected.clock,
            expected.device
        );
    }
    let _ = diagnose;
    if expected.frozen {
        good &= bulk_retry::frozen(&mut cpu, &mut device);
    } else if expected.trace {
        let snapshot = cpu.clone();
        let saved_device = device.clone();
        good &= cpu.step_instruction(&mut device) == Err(TimingFault::PendingTrace);
        good &= cpu == snapshot && device == saved_device;
    }
    good
}
