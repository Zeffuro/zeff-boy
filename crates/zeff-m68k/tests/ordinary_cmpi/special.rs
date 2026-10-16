use super::*;
use model::{Spec, access, status};
use oracle::Count;
use plan::{Action, Plan};

pub(super) fn run() -> (u32, u32) {
    let mut total = Count::default();
    for (opcode, ccr) in [(0x003c, 0x1f), (0x023c, 0x11), (0x0a3c, 0x0e)] {
        for supervisor in [false, true] {
            let mut s = Spec::new(1, 7, 4, supervisor);
            s.family = opcode & 0xff00;
            s.immediate = 0xff;
            s.r = status(s.r, if supervisor { 0x2511 } else { 0x0511 });
            assert_eq!(s.opcode(), opcode);
            let mut bus = device::Device::new(0);
            bus.word(0x1000, opcode);
            bus.word(0x1002, 0xff);
            bus.word(0x1004, 0x4e71);
            bus.word(0x1006, 0x7201);
            let mut r = status(s.r, s.r.status_register() & 0xffe0 | ccr);
            let published = r;
            r.pc = 0x1004;
            let p = Plan {
                actions: vec![
                    Action::Read(access(s.r, 0x1004, false, 2), 0x4e71),
                    Action::Idle(6),
                    Action::State(published),
                    Action::Idle(2),
                    Action::Read(access(s.r, 0x1004, false, 2), 0x4e71),
                    Action::Read(access(s.r, 0x1006, false, 2), 0x7201),
                    Action::Commit(r, [0x4e71, 0x7201]),
                ],
                address: 0,
                operand: 0,
                immediate: 0xff,
                final_read: 2,
            };
            for path in 0..3 {
                let mut b = bus.clone();
                if path == 1 {
                    b.fail = Some(2);
                } else if path == 2 {
                    capacity::blocked(&p, &mut b, 2);
                }
                let good = oracle::verify(s, &p, &mut b, total.bad < 3);
                total.count += 1;
                total.bad += u32::from(!good);
            }
        }
    }
    total.result()
}
