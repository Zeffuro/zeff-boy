use super::*;
use device::{Device, Event};
use model::{BUDGET, Spec};
use plan::{Action, Plan};

#[derive(Default)]
pub(super) struct Count {
    pub count: u32,
    pub bad: u32,
}
impl Count {
    pub fn case(&mut self, s: Spec, bus: Device) {
        let good = run(s, bus, self.bad < 3);
        self.count += 1;
        self.bad += u32::from(!good);
    }
    pub fn result(self) -> (u32, u32) {
        (self.count, self.bad)
    }
    pub fn merge(&mut self, other: (u32, u32)) {
        self.count += other.0;
        self.bad += other.1;
    }
}
pub(super) fn run(s: Spec, mut bus: Device, diagnose: bool) -> bool {
    verify(s, &plan::build(s, &bus), &mut bus, diagnose)
}
pub(super) fn verify(s: Spec, p: &Plan, bus: &mut Device, diagnose: bool) -> bool {
    let start = bus.clock;
    let mut cpu = PrefetchCpu::from_prefetched(s.r, s.queue(), CpuClocks(start)).unwrap();
    let mut expected_bus = bus.clone();
    let mut r = s.r;
    let mut queue = s.queue();
    let mut result = Ok(TimedStep {
        opcode: s.opcode(),
        clocks: 0,
        exception: None,
    });
    let mut committed = false;
    for action in &p.actions {
        match *action {
            Action::State(next) => r = next,
            Action::Commit(next, words) => {
                r = next;
                queue = words;
                committed = true;
            }
            Action::Reserve => {
                if expected_bus.clock.checked_add(BUDGET).is_none() {
                    result = Err(TimingFault::ClockOverflow);
                    break;
                }
            }
            Action::Idle(clocks) => {
                if let Some(end) = expected_bus.clock.checked_add(u64::from(clocks)) {
                    expected_bus
                        .events
                        .push(Event::Idle(expected_bus.clock, clocks));
                    expected_bus.clock = end;
                } else {
                    result = Err(TimingFault::ClockOverflow);
                    break;
                }
            }
            Action::Read(access, value) | Action::Write(access, value) => {
                if access.lanes == ByteLanes::Both && access.logical_address & 1 != 0 {
                    result = Err(TimingFault::AddressError {
                        address: access.logical_address,
                    });
                    break;
                }
                if expected_bus.clock.checked_add(BUDGET).is_none() {
                    result = Err(TimingFault::ClockOverflow);
                    break;
                }
                let write = matches!(action, Action::Write(..));
                if !write
                    && let Some((at, address, new)) = expected_bus.mutation
                    && at == expected_bus.count
                {
                    expected_bus.word(address, new);
                    expected_bus.mutation = None;
                }
                let wait = expected_bus.waits[expected_bus.count];
                let failed = expected_bus.fail == Some(expected_bus.count);
                if write {
                    expected_bus
                        .events
                        .push(Event::Write(access, expected_bus.clock, value, wait));
                    if !failed {
                        let old = model::sample(&expected_bus.words, access.logical_address);
                        let new = match access.lanes {
                            ByteLanes::Upper => old & 0xff | value & 0xff00,
                            ByteLanes::Lower => old & 0xff00 | value & 0xff,
                            ByteLanes::Both => value,
                        };
                        expected_bus.word(access.logical_address, new);
                    }
                } else {
                    assert_eq!(
                        value,
                        model::sample(&expected_bus.words, access.logical_address)
                    );
                    expected_bus
                        .events
                        .push(Event::Read(access, expected_bus.clock, value, wait));
                }
                expected_bus.count += 1;
                expected_bus.clock += 4 + u64::from(wait);
                if failed {
                    result = Err(TimingFault::Bus {
                        access,
                        direction: if write {
                            Direction::Write
                        } else {
                            Direction::Read
                        },
                        source: "host",
                    });
                    break;
                }
            }
        }
    }
    if let Ok(step) = &mut result {
        assert!(committed);
        step.clocks = expected_bus.clock - start;
    }
    let actual = cpu.step_instruction(bus);
    let pending = result.is_ok() && s.r.status_register() & 0x8000 != 0;
    let mut good = actual == result
        && cpu.registers == r
        && cpu.queued_words() == Some(queue)
        && cpu.clock().0 == expected_bus.clock
        && *bus == expected_bus
        && cpu.trace_pending() == pending
        && !cpu.stopped()
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues;
    if !good && diagnose {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary CMPI bulk {s:?} actual={actual:?} expected={result:?} regs={:?}/{r:?} queue={:?}/{queue:?} clock={}/{} device={bus:?}/{expected_bus:?}",
            cpu.registers,
            cpu.queued_words(),
            cpu.clock().0,
            expected_bus.clock
        );
    }
    let _ = diagnose;
    if result.is_err() {
        let frozen = cpu.clone();
        let device = bus.clone();
        good &= cpu.step_instruction(bus) == Err(TimingFault::Frozen)
            && cpu.step_boundary(bus) == Err(TimingFault::Frozen)
            && cpu.prime(bus) == Err(TimingFault::Frozen)
            && cpu.service_pending_trace(bus) == Err(TimingFault::Frozen)
            && cpu == frozen
            && *bus == device;
    } else if pending {
        let saved = cpu.clone();
        let device = bus.clone();
        good &= cpu.step_instruction(bus) == Err(TimingFault::PendingTrace)
            && cpu == saved
            && *bus == device;
    }
    good
}
