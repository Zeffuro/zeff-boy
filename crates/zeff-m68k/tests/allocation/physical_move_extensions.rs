use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn run(memory: &mut MemoryBus, form: (u16, usize), failure: Option<(usize, u8)>, odd: bool) {
    let (size, family) = form;
    let field = [
        0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x140, 0x180, 0x1c0, 0x3c0,
    ][family];
    let opcode = size << 12 | field;
    let indexed = matches!(family, 1 | 5 | 8);
    let two_extensions = matches!(family, 3 | 10) || family == 6 && size == 2;
    let memory_operand = family != 6;
    let count = 1
        + usize::from(two_extensions)
        + if memory_operand {
            if size == 2 { 2 } else { 1 }
        } else {
            0
        }
        + 1;
    let address = 0x3000 + u32::from(odd);
    let extension = match family {
        0 | 7 | 3 | 10 => 0,
        1 | 5 | 8 => 0x5800,
        2 | 9 => address as u16,
        4 => (address - 0x2002) as u16,
        6 => 0x8001,
        _ => unreachable!(),
    };
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    registers.a[0] = address;
    registers.d[0] = 0x8000_0001;
    registers.d[5] = if family == 5 { address - 0x2002 } else { 0 };
    registers.set_status_register(0x001f);
    for target in [0x3000, 0x3002] {
        memory.word(target, 0x8001);
    }
    memory.word(
        0x2004,
        if matches!(family, 3 | 10) {
            address as u16
        } else if family == 6 && size == 2 {
            1
        } else {
            0x4e71
        },
    );
    memory.word(0x2006, if two_extensions { 0x4e71 } else { 0x7201 });
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
    let idle = u64::from(indexed) * 2;
    if odd {
        let prefix = 1 + usize::from(two_extensions);
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
        assert_eq!(bus.transfers, count);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == idle + count as u64 * 7)
        );
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn extension_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for size in [1, 3, 2] {
            for family in 0..11 {
                let two = matches!(family, 3 | 10) || family == 6 && size == 2;
                let count = 2
                    + usize::from(two)
                    + if family == 6 {
                        0
                    } else if size == 2 {
                        2
                    } else {
                        1
                    };
                run(memory, (size, family), None, false);
                checked += 1;
                for at in 0..count {
                    for kind in 0..3 {
                        run(memory, (size, family), Some((at, kind)), false);
                        checked += 1;
                    }
                }
                if size != 1 && family != 6 {
                    run(memory, (size, family), None, true);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 39_200);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
