use super::*;
use bulk_device::Device;
use bulk_model::{Action, Phase, Plan, Spec, access, sample, set_active};

struct Builder {
    r: Registers,
    words: BTreeMap<u32, u16>,
    mutation: Option<(usize, u32, u16)>,
    actions: Vec<Action>,
    phases: Vec<Phase>,
}
impl Builder {
    fn transfer(
        &mut self,
        address: u32,
        direction: Direction,
        capture: (u32, u16, InstructionPhase),
        before: Registers,
        write: Option<u16>,
    ) -> u16 {
        if let Some((n, a, v)) = self.mutation
            && n == self.phases.len()
        {
            self.words.insert(a & 0x00ff_fffe, v);
        }
        let value = write.unwrap_or_else(|| sample(&self.words, address));
        let kind = if matches!(
            capture.2,
            InstructionPhase::ExtensionFirst
                | InstructionPhase::ExtensionSecond
                | InstructionPhase::OperandRefill
        ) {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        };
        let fc = if self.r.status_register() & 0x2000 == 0 {
            1
        } else {
            5
        } + u8::from(kind == AccessKind::Prefetch);
        let t = Phase {
            access: access(address, fc, kind),
            direction,
            pc: capture.0,
            ird: capture.1,
            phase: capture.2,
            before,
            during: self.r,
            value,
        };
        self.actions.push(Action::Transfer(t));
        self.phases.push(t);
        value
    }
}
pub(super) fn plan(s: Spec, bus: &Device) -> Plan {
    let mut b = Builder {
        r: s.r,
        words: bus.words.clone(),
        mutation: bus.mutation,
        actions: Vec::new(),
        phases: Vec::new(),
    };
    if s.idle() {
        b.actions.push(Action::Idle(2));
    }
    let p = s.r.pc;
    let mut low = s.low;
    let mut retained = s.queued();
    for n in 0..s.extensions() {
        let tag = if n == 0 {
            InstructionPhase::ExtensionFirst
        } else {
            InstructionPhase::ExtensionSecond
        };
        let capture_pc = p.wrapping_add(if n == 1 {
            6
        } else if s.mode == 5 {
            4
        } else {
            2
        });
        retained = b.transfer(
            p.wrapping_add(4 + n as u32 * 2),
            Direction::Read,
            (capture_pc, s.opcode(), tag),
            b.r,
            None,
        );
        if s.mode == 6 && n == 0 {
            low = retained;
        }
    }
    let address = s.address(low);
    let operand = if s.mode == 7 {
        s.r.d[s.reg]
    } else {
        let before = b.r;
        if s.mode == 2 {
            set_active(&mut b.r, s.reg, address);
        } else if s.mode == 1 {
            set_active(&mut b.r, s.reg, address.wrapping_add(2));
        }
        u32::from(b.transfer(
            address,
            Direction::Read,
            (
                s.operand_pc(),
                s.opcode(),
                InstructionPhase::OperandReadHigh,
            ),
            before,
            None,
        ))
    };
    let before = b.r;
    if s.mode == 7 {
        b.r.d[s.reg] = (b.r.d[s.reg] & 0xffff0000) | u32::from(b.r.status_register());
    }
    let capture_pc = s.canonical().wrapping_add(2);
    let fetched = b.transfer(
        capture_pc,
        Direction::Read,
        (capture_pc, s.opcode(), InstructionPhase::OperandRefill),
        before,
        None,
    );
    if s.mode == 7 {
        b.actions.push(Action::Idle(2));
    } else {
        b.transfer(
            address,
            Direction::Write,
            (capture_pc, retained, InstructionPhase::OperandWriteHigh),
            b.r,
            Some(b.r.status_register()),
        );
    }
    let mut r = b.r;
    r.pc = s.canonical();
    Plan {
        actions: b.actions,
        phases: b.phases,
        final_r: r,
        queue: [retained, fetched],
        address,
        operand,
    }
}
