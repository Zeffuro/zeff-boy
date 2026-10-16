use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn transfers(long: bool, family: usize) -> usize {
    let words = if long { 2 } else { 1 };
    match family {
        0 | 1 => 1,
        2..=4 => words + 1,
        5..=7 | 9 | 10 => words + 2,
        8 => words + 3,
        11 => words + 1,
        _ => unreachable!(),
    }
}

fn run(memory: &mut MemoryBus, long: bool, family: usize, failure: Option<(usize, u8)>, odd: bool) {
    let source = [
        0, 8, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b, 0x3c,
    ][family];
    let opcode = (if long { 2 } else { 3 }) << 12 | 0x640 | source;
    let address = 0x3000 + u32::from(odd);
    let indexed = matches!(family, 6 | 10);
    let idle = u64::from(indexed || family == 4) * 2;
    let two = family == 8 || family == 11 && long;
    let extension = match family {
        0..=4 => 0x4e71,
        5 | 8 => 0,
        6 | 10 => 0x5800,
        7 => address as u16,
        9 => (address - 0x2002) as u16,
        11 => 0x8001,
        _ => unreachable!(),
    };
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    registers.a[0] = address
        + if family == 4 {
            if long { 4 } else { 2 }
        } else {
            0
        };
    registers.d[0] = 0x8000_0001;
    registers.d[5] = if family == 10 { address - 0x2002 } else { 0 };
    registers.set_status_register(0x001f);
    memory.word(0x3000, 0x8001);
    memory.word(0x3002, 1);
    memory.word(
        0x2004,
        if family == 8 {
            address as u16
        } else if family == 11 && long {
            1
        } else {
            0x4e71
        },
    );
    memory.word(0x2006, if two { 0x4e71 } else { 0x7201 });
    memory.word(0x2008, 0x7201);
    let mut cpu =
        PrefetchCpu::from_prefetched(registers, [opcode, extension], CpuClocks(17)).unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        odd,
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    if odd {
        let prefix = if family >= 5 { 1 + usize::from(two) } else { 0 };
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
            if clocks == idle + prefix as u64 * 7 + 69)
        );
        assert_eq!(bus.transfers, prefix);
    } else if let Some((at, kind)) = failure {
        assert_eq!(bus.transfers, at + 1);
        match kind {
            0 => assert!(result.is_err()),
            1 => assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
                if clocks == idle + (at as u64 + 1) * 7 + 61)
            ),
            _ => assert!(matches!(
                result,
                Err(InstructionFault::TransitionTimingUnavailable { .. })
            )),
        }
    } else {
        let count = transfers(long, family);
        assert_eq!(bus.transfers, count);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == idle + count as u64 * 7)
        );
        assert_eq!(cpu.registers.status_register(), 0x001f);
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn movea_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for long in [false, true] {
            for family in 0..12 {
                run(memory, long, family, None, false);
                checked += 1;
                for at in 0..transfers(long, family) {
                    for kind in 0..3 {
                        run(memory, long, family, Some((at, kind)), false);
                        checked += 1;
                    }
                }
                if (2..=10).contains(&family) {
                    run(memory, long, family, None, true);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 24_600);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
