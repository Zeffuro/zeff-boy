use super::device::{Device, access};
use super::fixed::{Event, Ram};
use super::model::{Case, Failure, Profile};
use super::oracle::Expected;
use super::{
    AccessKind, Direction, InstructionPhase, PhysicalInstructionStep, Registers, TimedStep,
};

const VALUE: u16 = 0x2700;

fn read(
    e: &mut Expected,
    b: &mut Device,
    address: u32,
    program: bool,
    phase: InstructionPhase,
) -> u16 {
    let a = access(
        address,
        e.r.supervisor(),
        program,
        if phase == InstructionPhase::OperandReadHigh {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        false,
    );
    let value = e.words.sample(a.bus_address);
    b.phases[e.count] = Some(phase);
    e.events.push(Event::Transfer {
        stage: 0,
        phase: Some(phase),
        access: a,
        direction: Direction::Read,
        value: Some(value),
        start: e.clock,
        elapsed: 4,
        kind: Failure::Complete,
        deposited: false,
    });
    e.scripts.push(0, Failure::Complete, 4);
    e.clock += 4;
    e.count += 1;
    e.counts[0] += 1;
    value
}

pub(super) fn prepare(word: u16) -> (Case, Device, Expected) {
    let mode = (word >> 3 & 7) as u8;
    let reg = usize::from(word & 7);
    assert!(
        word & 0xffc0 == 0x46c0 && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 4)
    );
    let immediate = mode == 7 && reg == 4;
    let relative = mode == 7 && matches!(reg, 2 | 3);
    let indexed = mode == 6 || mode == 7 && reg == 3;
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0x1111_1111 * (i as u32 + 1));
    r.a = std::array::from_fn(|i| 0x3000 + i as u32 * 0x1000);
    r.usp = 0xb000;
    r.ssp = 0xa000;
    r.pc = 0x2000;
    r.set_status_register(0x251f);
    let extensions = if immediate || mode >= 5 {
        if mode == 7 && reg == 1 { 2 } else { 1 }
    } else {
        0
    };
    let retained = if immediate {
        VALUE
    } else if mode == 7 && reg == 1 {
        0
    } else if mode == 7 && reg == 0 {
        0x1000
    } else if mode == 7 && reg == 2 {
        (0x1000u32.wrapping_sub(r.pc + 2)) as u16
    } else if indexed {
        0x3010
    } else if mode == 5 {
        16
    } else {
        0x7001
    };
    let ea = if mode == 7 && reg == 3 {
        r.pc + 20
    } else {
        0x1000
    };
    if mode == 0 {
        r.d[reg] = r.d[reg] & 0xffff_0000 | u32::from(VALUE);
    }
    if indexed {
        r.d[3] = 2;
    }
    if matches!(mode, 2..=6) {
        r.set_address_register(
            reg,
            match mode {
                4 => ea + 2,
                5 => ea - 16,
                6 => ea - 18,
                _ => ea,
            },
        );
    }
    let c = Case {
        word,
        p: Profile {
            clear: false,
            bits: 16,
            mode,
            register: reg,
        },
        r,
        retained,
        low: 0x1000,
        operand: VALUE,
        next: 0x7001,
        last: 0x4e71,
        start: 0,
        handler: 0x4000,
        reset_handler: 0x7000,
        reset_ssp: 0x90000,
    };
    let anchor = r.pc + 2 + extensions * 2;
    let mut b = Device::seed(super::seed::make(0x4200, true, 0));
    b.profile = c.p;
    b.words = Ram::default();
    b.words.insert(r.pc, word);
    if extensions > 0 {
        b.words.insert(r.pc + 2, retained);
    }
    if extensions == 2 {
        b.words.insert(r.pc + 4, 0x1000);
    }
    b.words.insert(anchor, c.next);
    b.words.insert(anchor + 2, c.last);
    for (address, value) in [
        (0, 9),
        (2, 0),
        (4, 0),
        (6, 0x7000),
        (8, 0),
        (10, 0x4000),
        (0x4000, 0x4e71),
        (0x4002, 0x7002),
        (0x7000, 0x4e71),
        (0x7002, 0x7002),
    ] {
        b.words.insert(address, value);
    }
    if mode != 0 && !immediate {
        b.words.insert(ea, VALUE);
    }
    let mut e = super::oracle::initial(c, &b);
    if mode == 4 || indexed {
        assert!(e.idle(2));
    }
    let source = if immediate {
        read(
            &mut e,
            &mut b,
            r.pc + 4,
            true,
            InstructionPhase::ImmediateFirst,
        );
        retained
    } else if mode == 0 {
        e.r.d[reg] as u16
    } else {
        if extensions > 0 {
            read(
                &mut e,
                &mut b,
                r.pc + 4,
                true,
                InstructionPhase::ExtensionFirst,
            );
            if extensions == 2 {
                read(
                    &mut e,
                    &mut b,
                    r.pc + 6,
                    true,
                    InstructionPhase::ExtensionSecond,
                );
            }
        }
        if matches!(mode, 3 | 4) {
            e.r.set_address_register(reg, ea + if mode == 3 { 2 } else { 0 });
        }
        read(
            &mut e,
            &mut b,
            ea,
            relative,
            InstructionPhase::OperandReadHigh,
        )
    };
    assert!(e.idle(2));
    e.r.set_status_register(source);
    assert!(e.idle(2));
    let first = read(&mut e, &mut b, anchor, true, InstructionPhase::StatusFirst);
    let last = read(
        &mut e,
        &mut b,
        anchor + 2,
        true,
        InstructionPhase::StatusSecond,
    );
    e.r.pc = anchor;
    e.pipeline_address = Some(anchor);
    e.queue = Some([first, last]);
    e.result = Ok(PhysicalInstructionStep::Instruction(TimedStep {
        opcode: word,
        clocks: e.clock,
        exception: None,
    }));
    (c, b, e)
}

pub(super) fn run(word: u16, mark: fn(bool)) -> bool {
    let (c, config, expected) = prepare(word);
    mark(true);
    let mut cpu =
        super::PrefetchCpu::from_prefetched(c.r, [c.word, c.retained], super::CpuClocks(c.start))
            .unwrap();
    let mut b = config;
    let mut copy = cpu.clone();
    let mut copied_bus = b.clone();
    let copied_result = super::check::step(&mut copy, &mut copied_bus);
    let result = super::check::step(&mut cpu, &mut b);
    mark(false);
    result == expected.result
        && copied_result == result
        && cpu == copy
        && b == copied_bus
        && super::check::verify(&cpu, &b, &expected)
        && super::check::verify(&copy, &copied_bus, &expected)
}

pub(super) fn mini() -> (u32, u32) {
    let rows = super::literal::status_records();
    let mut bad = 0;
    for row in &rows {
        let (c, config, expected) = prepare(row.get("word").u16());
        let initial = row.get("initial");
        let good = c.r == super::mini::registers(initial.get("registers"))
            && [c.word, c.retained] == super::mini::queue(initial.get("queue")).unwrap()
            && super::mini::ram_matches(&config.words, initial.get("word_RAM"))
            && super::mini::status_matches(&expected, row);
        bad += u32::from(!good);
    }
    (rows.len() as u32, bad)
}
