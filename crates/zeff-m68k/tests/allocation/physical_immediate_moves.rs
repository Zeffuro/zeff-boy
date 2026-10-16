use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn count(size: u16, family: usize) -> usize {
    3 + 2 * usize::from(size == 2) + usize::from(family >= 3) + usize::from(family == 6)
}

fn run(memory: &mut MemoryBus, size: u16, family: usize, failure: Option<(usize, u8)>, odd: bool) {
    let field = [0x80, 0xc0, 0x100, 0x140, 0x180, 0x1c0, 0x3c0][family];
    let opcode = size << 12 | field | 0x3c;
    let address = 0x3000 + u32::from(odd);
    let anchor = if size == 2 { 0x2006 } else { 0x2004 };
    let extensions = usize::from(family >= 3) + usize::from(family == 6);
    let end = anchor + extensions * 2;
    let retained = match family {
        0..=2 => 0x4e71,
        3 | 6 => 0,
        4 => 0x5800,
        5 => address as u16,
        _ => unreachable!(),
    };
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    registers.a[0] = address
        + if family == 2 {
            if size == 2 {
                4
            } else if size == 3 {
                2
            } else {
                1
            }
        } else {
            0
        };
    registers.d[0] = 0xdead_beef;
    registers.set_status_register(0x001f);
    memory.word(0x2004, 1);
    memory.word(anchor, retained);
    if family == 6 {
        memory.word(anchor + 2, address as u16);
    }
    memory.word(end, 0x4e71);
    memory.word(end + 2, 0x7201);
    let mut cpu = PrefetchCpu::from_prefetched(
        registers,
        [opcode, if size == 2 { 0x8000 } else { 1 }],
        CpuClocks(17),
    )
    .unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        odd,
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    let idle = u64::from(family == 4) * 2;
    if odd {
        let prefix = 1 + usize::from(size == 2) + extensions + usize::from(family == 2);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
            if clocks == idle + prefix as u64 * 7 + 69)
        );
        assert_eq!(bus.transfers, prefix);
    } else if let Some((at, kind)) = failure {
        assert_eq!(bus.transfers, at + 1);
        let reached_idle = u64::from(family == 4 && at > usize::from(size == 2)) * 2;
        match kind {
            0 => assert!(result.is_err()),
            1 => assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
                if clocks == reached_idle + (at as u64 + 1) * 7 + 61)
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
        assert_eq!(cpu.registers.d, registers.d);
        assert_eq!(cpu.registers.pc, end as u32);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn immediate_steps(memory: &mut MemoryBus) {
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
            for family in 0..7 {
                run(memory, size, family, None, false);
                checked += 1;
                for at in 0..count(size, family) {
                    for kind in 0..3 {
                        run(memory, size, family, Some((at, kind)), false);
                        checked += 1;
                    }
                }
                if size != 1 {
                    run(memory, size, family, None, true);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 31_100);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
