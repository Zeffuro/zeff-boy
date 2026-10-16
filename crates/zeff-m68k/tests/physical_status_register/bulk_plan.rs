use super::*;
use bulk_device::Device;
use bulk_model::{Action, Phase, Plan, Spec, access};

pub(super) fn plan(s: Spec, bus: &Device) -> Plan {
    let mut actions = Vec::new();
    let mut phases = Vec::new();
    let mut words = bus.words.clone();
    let mut transfer = |r: Registers, address: u32, ird: u16, tag: InstructionPhase| {
        if let Some((at, a, v)) = bus.mutation
            && at == phases.len()
        {
            words.insert(a & 0x00ff_fffe, v);
        }
        let value = words
            .get(&(address & 0x00ff_fffe))
            .copied()
            .unwrap_or(bus.default_word);
        let phase = Phase {
            access: access(
                address,
                if r.supervisor() { 6 } else { 2 },
                AccessKind::Prefetch,
            ),
            direction: Direction::Read,
            pc: address,
            ird,
            phase: tag,
            before: r,
            during: r,
            value,
        };
        phases.push(phase);
        value
    };
    if s.immediate {
        transfer(
            s.r,
            s.r.pc.wrapping_add(4),
            s.opcode(),
            InstructionPhase::ImmediateFirst,
        );
    }
    let r = s.published();
    let first = transfer(r, s.anchor(), s.opcode(), InstructionPhase::StatusFirst);
    let last = transfer(
        r,
        s.anchor().wrapping_add(2),
        first,
        InstructionPhase::StatusSecond,
    );
    if s.immediate {
        actions.push(Action::Transfer(phases[0]));
    }
    actions.push(Action::Idle(2));
    actions.push(Action::Publish(r));
    actions.push(Action::Idle(2));
    for t in phases.iter().skip(usize::from(s.immediate)) {
        actions.push(Action::Transfer(*t));
    }
    let mut final_r = r;
    final_r.pc = s.anchor();
    Plan {
        actions,
        phases,
        final_r,
        queue: [first, last],
    }
}
