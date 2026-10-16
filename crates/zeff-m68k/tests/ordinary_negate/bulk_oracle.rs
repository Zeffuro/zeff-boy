use super::*;
use bulk_device::{Device, Event};
use bulk_model::{BUDGET, Spec};
use bulk_plan::{Action, Plan};

#[derive(Clone, Debug)]
pub(super) struct Expected {
    pub r: Registers,
    pub queue: [u16; 2],
    pub clock: u64,
    pub result: Result<TimedStep, TimingFault<usize>>,
    pub trace: bool,
    pub frozen: bool,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub count: usize,
    pub device: Device,
}

pub(super) fn expect(spec: Spec, original: &Device, plan: &Plan) -> Expected {
    let mut device = original.clone();
    let mut r = spec.r;
    let mut queue = spec.queue();
    let mut result = Ok(TimedStep {
        opcode: spec.opcode(),
        clocks: 0,
        exception: None,
    });
    let mut committed = false;
    for action in &plan.actions {
        match *action {
            Action::State(next) => r = next,
            Action::Commit(next, words) => {
                r = next;
                queue = words;
                committed = true;
            }
            Action::Reserve => {
                if device.clock.checked_add(BUDGET).is_none() {
                    result = Err(TimingFault::ClockOverflow);
                    break;
                }
            }
            Action::Idle(clocks) => {
                if let Some(end) = device.clock.checked_add(u64::from(clocks)) {
                    device.events.push(Event::Idle(device.clock, clocks));
                    device.clock = end;
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
                if device.clock.checked_add(BUDGET).is_none() {
                    result = Err(TimingFault::ClockOverflow);
                    break;
                }
                if let Some((at, location, value)) = device.mutation
                    && at == device.count
                {
                    device.word(location, value);
                    device.mutation = None;
                }
                let write = matches!(action, Action::Write(..));
                let failed = device.fail == Some(device.count);
                let wait = device.waits[device.count];
                if write {
                    device
                        .events
                        .push(Event::Write(access, device.clock, value, wait));
                    if !failed || device.deposit_failed {
                        let old = device.words.get(&access.bus_address).copied().unwrap_or(0);
                        device.word(
                            access.logical_address,
                            match access.lanes {
                                ByteLanes::Both => value,
                                ByteLanes::Upper => old & 0xff | value & 0xff00,
                                ByteLanes::Lower => old & 0xff00 | value & 0xff,
                            },
                        );
                    }
                } else {
                    device.valid &=
                        bulk_model::sample(&device.words, access.logical_address) == value;
                    device
                        .events
                        .push(Event::Read(access, device.clock, value, wait));
                }
                let index = device.count;
                device.count += 1;
                device.clock += 4 + u64::from(wait);
                if failed {
                    result = Err(TimingFault::Bus {
                        access,
                        direction: if write {
                            Direction::Write
                        } else {
                            Direction::Read
                        },
                        source: index,
                    });
                    break;
                }
            }
        }
    }
    if let Ok(step) = &mut result {
        assert!(committed);
        step.clocks = device.clock - original.clock;
    }
    Expected {
        r,
        queue,
        clock: device.clock,
        trace: result.is_ok() && spec.r.status_register() & 0x8000 != 0,
        frozen: result.is_err(),
        words: device.words.clone(),
        events: device.events.clone(),
        count: device.count,
        result,
        device,
    }
}
