use super::*;
use InstructionPhase::{
    ExtensionFirst as E1, ExtensionSecond as E2, OperandReadHigh as H, StatusFirst as R1,
    StatusSecond as R2,
};

pub(super) fn post(context: usize, operand: u16, new_sr: u16) -> Case {
    let opcode = if context == 2 { 0x46df } else { 0x44df };
    let mut r = registers(if context == 0 { 0x071b } else { 0x271b });
    if context == 0 {
        r.usp = 0x3000;
        r.ssp = 0x5000;
    }
    let mut source = r;
    if context == 0 {
        source.usp = 0x3002;
    } else {
        source.ssp = 0x3002;
    }
    let published = status(source, new_sr);
    let old_fc = if context == 0 { 1 } else { 5 };
    let new_fc = if new_sr & 0x2000 == 0 { 2 } else { 6 };
    let mut c = Case::new(opcode, 0x4afc, r);
    c.read(
        0,
        0x3000,
        old_fc,
        (AccessKind::Data, 0x1002, opcode, H),
        (source, operand),
    );
    c.read(
        8,
        0x1002,
        new_fc,
        (AccessKind::Prefetch, 0x1002, opcode, R1),
        (published, 0x4e71),
    );
    c.read(
        12,
        0x1004,
        new_fc,
        (AccessKind::Prefetch, 0x1004, 0x4e71, R2),
        (published, 0x7000),
    );
    c.idles = vec![(4, 2), (6, 2)];
    c.final_r = published;
    c.final_r.pc = 0x1002;
    c.duration = 16;
    c
}
fn pc_displacement() -> Case {
    let r = registers(0x2700);
    let published = status(r, 0x001f);
    let mut c = Case::new(0x46fa, 0x0010, r);
    c.read(
        0,
        0x1004,
        6,
        (AccessKind::Prefetch, 0x1002, 0x46fa, E1),
        (r, 0x4afc),
    );
    c.read(
        4,
        0x1012,
        6,
        (AccessKind::Data, 0x1002, 0x46fa, H),
        (r, 0x001f),
    );
    c.read(
        12,
        0x1004,
        2,
        (AccessKind::Prefetch, 0x1004, 0x46fa, R1),
        (published, 0x4e71),
    );
    c.read(
        16,
        0x1006,
        2,
        (AccessKind::Prefetch, 0x1006, 0x4e71, R2),
        (published, 0x7000),
    );
    c.bus.word(0x1004, 0x4afc);
    c.bus.mutation = Some((2, 0x1004, 0x4e71));
    c.idles = vec![(8, 2), (10, 2)];
    c.final_r = published;
    c.final_r.pc = 0x1004;
    c.duration = 20;
    c
}
fn direct(context: usize, trace: bool) -> Case {
    let old_sr = if context == 0 {
        0x071b
    } else if trace {
        0x2700
    } else {
        0x271b
    };
    let opcode = if trace { 0x46d0 } else { 0x44d0 };
    let mut r = registers(old_sr);
    r.a[0] = 0x3000;
    let new_sr = if trace {
        0xa71f
    } else if context == 0 {
        0x071f
    } else {
        0x271f
    };
    let published = status(r, new_sr);
    let fc = if context == 0 { 1 } else { 5 };
    let pfc = if context == 0 { 2 } else { 6 };
    let mut c = Case::new(opcode, 0x4e71, r);
    c.read(
        0,
        0x3000,
        fc,
        (AccessKind::Data, 0x1002, opcode, H),
        (r, if trace { 0xffff } else { 0x001f }),
    );
    c.read(
        8,
        0x1002,
        pfc,
        (AccessKind::Prefetch, 0x1002, opcode, R1),
        (published, if trace { 0x4e71 } else { 0xffff }),
    );
    c.read(
        12,
        0x1004,
        pfc,
        (
            AccessKind::Prefetch,
            0x1004,
            if trace { 0x4e71 } else { 0xffff },
            R2,
        ),
        (published, 0x7000),
    );
    c.idles = vec![(4, 2), (6, 2)];
    c.final_r = published;
    c.final_r.pc = 0x1002;
    c.duration = 16;
    c
}
fn odd_user() -> (Case, Transfer) {
    let mut r = registers(0x071b);
    r.usp = 0x3001;
    r.ssp = 0x5000;
    let c = Case::new(0x44df, 0x4afc, r);
    r.usp = 0x3003;
    let t = Transfer {
        at: 0,
        access: access(0x3001, 1, AccessKind::Data),
        pc: 0x1002,
        ird: 0x44df,
        phase: H,
        r,
        value: 0,
    };
    (c, t)
}
fn odd_absolute() -> (Case, Transfer) {
    let r = registers(0x2700);
    let mut c = Case::new(0x46f9, 0x1234, r);
    c.read(
        0,
        0x1004,
        6,
        (AccessKind::Prefetch, 0x1002, 0x46f9, E1),
        (r, 0x0001),
    );
    c.read(
        4,
        0x1006,
        6,
        (AccessKind::Prefetch, 0x1006, 0x46f9, E2),
        (r, 0x4afc),
    );
    let t = Transfer {
        at: 8,
        access: access(0x1234_0001, 5, AccessKind::Data),
        pc: 0x1006,
        ird: 0x46f9,
        phase: H,
        r,
        value: 0,
    };
    (c, t)
}
fn indexed(context: usize) -> Case {
    let opcode = if context == 2 { 0x46f0 } else { 0x44f0 };
    let mut r = registers(if context == 0 { 0x071b } else { 0x271b });
    r.a[0] = 0x10000;
    r.d[3] = 0x8002;
    let new_sr = [0x071f, 0x271f, 0xa71f][context];
    let published = status(r, new_sr);
    let pfc = if context == 0 { 2 } else { 6 };
    let dfc = if context == 0 { 1 } else { 5 };
    let mut c = Case::new(opcode, 0x37f0, r);
    c.read(
        2,
        0x1004,
        pfc,
        (AccessKind::Prefetch, 0x1002, opcode, E1),
        (r, 0x4e71),
    );
    c.read(
        6,
        0x7ff2,
        dfc,
        (AccessKind::Data, 0x1002, opcode, H),
        (r, 0xa71f),
    );
    c.read(
        14,
        0x1004,
        pfc,
        (AccessKind::Prefetch, 0x1004, opcode, R1),
        (published, 0x4e71),
    );
    c.read(
        18,
        0x1006,
        pfc,
        (AccessKind::Prefetch, 0x1006, 0x4e71, R2),
        (published, 0x7000),
    );
    c.idles = vec![(0, 2), (10, 2), (12, 2)];
    c.final_r = published;
    c.final_r.pc = 0x1004;
    c.duration = 22;
    c
}
fn gated() -> bool {
    let mut r = registers(0x071b);
    r.usp = 0x3001;
    r.ssp = 0x5000;
    let mut c = Case::new(0x46df, 0x4afc, r);
    let mut cpu = c.seed();
    let before = cpu.clone();
    let bus = c.bus.clone();
    cpu.step_physical_instruction(&mut c.bus)
        == Err(InstructionFault::Execution(
            TimingFault::UnsupportedInstruction {
                pc: 0x1000,
                opcode: 0x46df,
            },
        ))
        && cpu == before
        && c.bus == bus
}
pub(super) fn record(count: &mut u32, bad: &mut u32, good: bool, label: &str) {
    *count += 1;
    *bad += u32::from(!good);
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("status literal {} {}", *count, label);
    }
    let _ = label;
}
pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for context in 0..2 {
        let sr = [0x0704, 0x2704][context];
        let f1 = if context == 0 {
            [0x1002, 0x0704, 0, 0x44df, 0x1002, 0x44d2, 0]
        } else {
            [0x1002, 0x2704, 0, 0x44df, 0x1002, 0x44d6, 0]
        };
        let f2 = if context == 0 {
            [0x1004, 0x0704, 0, 0x4e71, 0x1004, 0x4e72, 0]
        } else {
            [0x1004, 0x2704, 0, 0x4e71, 0x1004, 0x4e76, 0]
        };
        for path in [
            Path::Complete,
            Path::Host(1),
            Path::Host(2),
            Path::Qualified(1, f1),
            Path::Qualified(2, f2),
        ] {
            record(
                &mut count,
                &mut bad,
                check::run(post(context, 0xffe4, sr), path),
                "L01",
            );
        }
    }
    for (operand, sr, frame) in [
        (
            0x5810,
            0x0010,
            [0x1002, 0x0010, 0, 0x46df, 0x1002, 0x46d2, 0],
        ),
        (
            0xffff,
            0xa71f,
            [0x1002, 0xa71f, 0, 0x46df, 0x1002, 0x46d6, 0],
        ),
    ] {
        for path in [Path::Complete, Path::Qualified(1, frame)] {
            record(
                &mut count,
                &mut bad,
                check::run(post(2, operand, sr), path),
                "L02",
            );
        }
    }
    for elapsed in [4, 8] {
        let (c, t) = odd_user();
        record(
            &mut count,
            &mut bad,
            check::run(
                c,
                Path::Odd(t, [0x1002, 0x071b, 0, 0x44df, 0x3001, 0x44d1, 0], elapsed),
            ),
            "L03",
        );
    }
    for elapsed in [4, 8] {
        let (c, t) = odd_absolute();
        record(
            &mut count,
            &mut bad,
            check::run(
                c,
                Path::Odd(t, [0x1006, 0x2700, 0, 0x46f9, 1, 0x46f5, 0x1234], elapsed),
            ),
            "L04",
        );
    }
    for path in [
        Path::Complete,
        Path::Host(2),
        Path::Qualified(3, [0x1006, 0x001f, 0, 0x4e71, 0x1006, 0x4e72, 0]),
    ] {
        record(
            &mut count,
            &mut bad,
            check::run(pc_displacement(), path),
            "L05",
        );
    }
    record(
        &mut count,
        &mut bad,
        check::run(direct(1, true), Path::Complete),
        "L06",
    );
    record(&mut count, &mut bad, gated(), "L07");
    for context in 0..2 {
        record(
            &mut count,
            &mut bad,
            check::run(direct(context, false), Path::Unknown(2)),
            "L08",
        );
    }
    capacity::witnesses(&mut count, &mut bad);
    for context in 0..3 {
        let frame = [
            [0x1006, 0x071f, 0, 0x4e71, 0x1006, 0x4e72, 0],
            [0x1006, 0x271f, 0, 0x4e71, 0x1006, 0x4e76, 0],
            [0x1006, 0xa71f, 0, 0x4e71, 0x1006, 0x4e76, 0],
        ][context];
        for path in [Path::Complete, Path::Host(0), Path::Qualified(3, frame)] {
            record(
                &mut count,
                &mut bad,
                check::run(indexed(context), path),
                "L14",
            );
        }
    }
    (count, bad + u32::from(count != 58))
}
