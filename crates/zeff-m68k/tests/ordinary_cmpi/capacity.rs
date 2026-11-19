use super::*;
use device::Device;
use model::BUDGET;
use plan::{Action, Plan};

pub(super) fn prefix(p: &Plan, bus: &Device, target: usize) -> u64 {
    let mut clock = 0;
    let mut index = 0;
    for action in &p.actions {
        match action {
            Action::Read(..) | Action::Write(..) => {
                if index == target {
                    return clock;
                }
                clock += 4 + u64::from(bus.waits[index]);
                index += 1;
            }
            Action::Idle(c) => clock += u64::from(*c),
            _ => {}
        }
    }
    panic!("callback index")
}
pub(super) fn callbacks(p: &Plan) -> usize {
    p.actions
        .iter()
        .filter(|a| matches!(a, Action::Read(..) | Action::Write(..)))
        .count()
}
pub(super) fn blocked(p: &Plan, bus: &mut Device, target: usize) {
    if target == 0 {
        bus.clock = u64::MAX - BUDGET + 1;
        return;
    }
    bus.waits[target - 1] = u32::MAX;
    bus.clock = u64::MAX - prefix(p, bus, target);
}
pub(super) fn exact(p: &Plan, bus: &mut Device, target: usize) {
    bus.clock = u64::MAX - BUDGET - prefix(p, bus, target);
    bus.waits[target] = u32::MAX;
}
pub(super) fn source_idle(p: &Plan, bus: &mut Device, last: usize, room: u64) {
    bus.clock = u64::MAX - BUDGET - prefix(p, bus, last) - room;
    bus.waits[last] = u32::MAX;
}
