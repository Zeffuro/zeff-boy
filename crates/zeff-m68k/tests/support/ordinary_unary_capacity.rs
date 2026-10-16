use super::*;
use bulk_check::Count;
use bulk_device::Device;
use bulk_model::{BUDGET, Spec};
use bulk_plan::{Action, Plan};

pub(super) fn prefix_time(plan: &Plan, device: &Device, target: usize) -> u64 {
    let (mut index, mut clocks) = (0, 0);
    for action in &plan.actions {
        match action {
            Action::Idle(n) => clocks += u64::from(*n),
            Action::Read(..) | Action::Write(..) => {
                if index == target {
                    break;
                }
                clocks += 4 + u64::from(device.waits[index]);
                index += 1;
            }
            _ => {}
        }
    }
    clocks
}

pub(super) fn remaining(plan: &Plan, device: &mut Device, target: usize, remaining: u64) {
    if target != 0 {
        device.waits[target - 1] = u32::MAX;
    }
    let prefix = prefix_time(plan, device, target);
    device.clock = u64::MAX - (prefix + remaining);
}

pub(super) fn blocked(plan: &Plan, device: &mut Device, target: usize) {
    remaining(plan, device, target, BUDGET - 1);
}

fn prepared(mut spec: Spec, bank: bool, ccr: u16, source: u32) -> (Spec, Device, Plan) {
    spec = spec.bank(bank, ccr);
    spec.operand = source;
    let (spec, device) = spec.seed();
    let plan = bulk_plan::build(spec, &device);
    (spec, device, plan)
}

pub(super) fn edges() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for ccr in [0, 31] {
                for source in [0, 0x8000] {
                    let (spec, device, plan) = prepared(base, bank, ccr, source);
                    for target in 0..spec.callbacks() {
                        for prefix_wait in [0, 3] {
                            for room in [BUDGET - 1, BUDGET, BUDGET + 1] {
                                let mut device = device.clone();
                                device.waits = [prefix_wait; 12];
                                device.waits[target] = u32::MAX;
                                remaining(&plan, &mut device, target, room);
                                count.planned(spec, device, plan.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn idle() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded()
        .into_iter()
        .filter(|s| matches!(s.mode, 4 | 6))
    {
        for bank in [false, true] {
            for ccr in [0, 31] {
                for source in [0, 0x8000] {
                    let (spec, device, plan) = prepared(base, bank, ccr, source);
                    for room in [0, 1, 2, BUDGET - 1, BUDGET, BUDGET + 1] {
                        let mut device = device.clone();
                        device.clock = u64::MAX - room;
                        count.planned(spec, device, plan.clone());
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn maximum_waits() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for ccr in [0, 31] {
                for source in [0, 0x8000] {
                    let (spec, device, plan) = prepared(base, bank, ccr, source);
                    for target in 0..spec.callbacks() {
                        for host in [false, true] {
                            let mut device = device.clone();
                            device.waits[target] = u32::MAX;
                            device.fail = host.then_some(target);
                            count.planned(spec, device, plan.clone());
                        }
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn exact_final() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for ccr in [0, 31] {
                for source in [0, 0x8000] {
                    let (spec, device, plan) = prepared(base, bank, ccr, source);
                    for wait in [0, 3] {
                        let mut device = device.clone();
                        device.waits = [wait; 12];
                        let last = spec.callbacks() - 1;
                        device.waits[last] = u32::MAX;
                        device.clock = u64::MAX - (prefix_time(&plan, &device, last) + BUDGET);
                        let expected = bulk_oracle::expect(spec, &device, &plan);
                        assert_eq!(expected.clock, u64::MAX);
                        assert!(expected.result.is_ok());
                        count.planned(spec, device, plan.clone());
                    }
                }
            }
        }
    }
    count.result()
}

pub(super) fn odd() -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for ccr in 0..32 {
                for source in bulk_model::values(base.size) {
                    let mut spec = base.bank(bank, ccr);
                    spec.operand = source;
                    let target = bulk_model::address(spec, spec.extension) | 1;
                    spec.target(target);
                    let (spec, device) = spec.seed();
                    let plan = bulk_plan::build(spec, &device);
                    assert_eq!(plan.address, target);
                    count.planned(spec, device, plan);
                }
            }
        }
    }
    count.result()
}
