use super::*;
use InstructionPhase::{
    ExtensionFirst as E1, ExtensionSecond as E2, ImmediateFirst as I1, ImmediateSecond as I2,
    OperandReadHigh as RH, OperandRefill as F,
};

pub(super) fn check(c: Case, actual: &[oracle::Transfer]) -> bool {
    let rows: &[(u32, u32, InstructionPhase, bool)] = match c.opcode() {
        0x0c00 | 0x0c40 => &[(0x1004, 0x1004, I1, false), (0x1006, 0x1006, F, true)],
        0x0c80 => &[
            (0x1004, 0x1002, I1, false),
            (0x1006, 0x1006, I2, false),
            (0x1008, 0x1008, F, true),
        ],
        0x0c17 | 0x0c1f => &[
            (0x1004, 0x1004, I1, false),
            (0x3001, 0x1004, RH, false),
            (0x1006, 0x1006, F, true),
        ],
        0x0c27 => &[
            (0x1004, 0x1004, I1, false),
            (0x3001, 0x1006, RH, false),
            (0x1006, 0x1006, F, true),
        ],
        0x0c2f | 0x0c37 => &[
            (0x1004, 0x1004, I1, false),
            (0x1006, 0x1004, E1, false),
            (0x3001, 0x1004, RH, false),
            (0x1008, 0x1008, F, true),
        ],
        0x0c38 => &[
            (0x1004, 0x1004, I1, false),
            (0x1006, 0x1006, E1, false),
            (0x3001, 0x1006, RH, false),
            (0x1008, 0x1008, F, true),
        ],
        0x0c39 => &[
            (0x1004, 0x1004, I1, false),
            (0x1006, 0x1004, E1, false),
            (0x1008, 0x1008, E2, false),
            (0x3001, 0x1008, RH, false),
            (0x100a, 0x100a, F, true),
        ],
        _ => return false,
    };
    rows.len() == actual.len()
        && rows.iter().zip(actual).all(|(&(a, pc, phase, flags), t)| {
            let mut r = c.r;
            if c.opcode() == 0x0c27 && matches!(phase, RH | F) {
                r.set_address_register(7, 0x3001);
            }
            if c.opcode() == 0x0c1f && matches!(phase, RH | F) {
                r.set_address_register(7, 0x3003);
            }
            if flags {
                r.set_status_register(if r.supervisor() { 0x201b } else { 0x1b });
            }
            let mut access = oracle::access(
                a,
                c.r.supervisor(),
                phase != RH,
                if phase == RH {
                    AccessKind::Data
                } else {
                    AccessKind::Prefetch
                },
            );
            if phase == RH {
                access.lanes = ByteLanes::Lower;
            }
            *t == oracle::Transfer {
                access,
                phase,
                registers: r,
                internal_pc: pc,
                ird: if flags && c.opcode() != 0x0c80 {
                    0x4e71
                } else {
                    c.opcode()
                },
            }
        })
}
