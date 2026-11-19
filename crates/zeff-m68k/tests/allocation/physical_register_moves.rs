use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn count(size: u16, family: usize) -> usize {
    if family < 2 {
        1
    } else {
        2 + usize::from(size == 2) + usize::from(family >= 5) + usize::from(family == 8)
    }
}

fn run(memory: &mut MemoryBus, size: u16, family: usize, failure: Option<(usize, u8)>, odd: bool) {
    let field = [0x600, 0x608, 0x8a, 0xca, 0x10a, 0x14a, 0x18a, 0x1ca, 0x3ca][family];
    let opcode = size << 12 | field;
    let address = 0x3000 + u32::from(odd);
    let extension = match family {
        0..=4 => 0x4e71,
        5 | 8 => 0,
        6 => 0x5800,
        7 => address as u16,
        _ => unreachable!(),
    };
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    registers.a[0] = address
        + if family == 4 {
            if size == 2 { 4 } else { 2 }
        } else {
            0
        };
    registers.a[2] = 0x8000_0001;
    if family == 1 {
        registers.a[0] = 0x8000_0001;
    }
    registers.d[0] = 0x8000_0001;
    registers.d[3] = 0xfeee_0000;
    registers.set_status_register(0x001f);
    memory.word(0x2004, if family == 8 { address as u16 } else { 0x4e71 });
    memory.word(0x2006, if family == 8 { 0x4e71 } else { 0x7201 });
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
    let idle = u64::from(family == 6) * 2;
    if odd {
        let prefix = usize::from(family == 4 || family >= 5) + usize::from(family == 8);
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
        assert_eq!(bus.transfers, count(size, family));
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == idle + count(size, family) as u64 * 7)
        );
        assert_eq!(
            cpu.registers.status_register(),
            if size == 2 { 0x18 } else { 0x10 }
        );
        if family < 2 {
            assert_eq!(
                cpu.registers.d[3],
                if size == 2 { 0x8000_0001 } else { 0xfeee_0001 }
            );
        }
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn register_steps(memory: &mut MemoryBus) {
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
            for family in 0..if size == 1 { 1 } else { 9 } {
                run(memory, size, family, None, false);
                checked += 1;
                for at in 0..count(size, family) {
                    for kind in 0..3 {
                        run(memory, size, family, Some((at, kind)), false);
                        checked += 1;
                    }
                }
                if family >= 2 {
                    run(memory, size, family, None, true);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 18_300);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
