use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn count(size: u16, source: usize) -> usize {
    extensions(source) + 3 + 2 * usize::from(size == 2)
}

fn extensions(source: usize) -> usize {
    if source == 6 {
        2
    } else {
        usize::from(source >= 3)
    }
}

fn run(
    memory: &mut MemoryBus,
    size: u16,
    source: usize,
    destination: u16,
    failure: Option<(usize, u8)>,
    odd: Option<bool>,
) {
    let field = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b][source];
    let opcode = size << 12 | 0x200 | destination << 6 | field;
    let stride = if size == 2 {
        4
    } else if size == 3 {
        2
    } else {
        1
    };
    let operand = 0x3000 + u32::from(odd == Some(false));
    let target = 0x4000 + u32::from(odd == Some(true));
    let end = 0x2002 + extensions(source) as u32 * 2;
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.set_status_register(0x001f);
    r.a[0] = if source == 4 {
        0x2000
    } else {
        operand + if source == 2 { stride } else { 0 }
    };
    r.a[1] = target + if destination == 4 { stride } else { 0 };
    r.d[5] = 0x1000;
    let extension = match source {
        0..=2 => 0x4e71,
        3 => 0,
        4 => 0x5800 | u16::from(odd == Some(false)),
        5 => operand as u16,
        6 => 0,
        7 => (operand - 0x2002) as u16,
        8 => 0x58fe | u16::from(odd == Some(false)),
        _ => unreachable!(),
    };
    memory.word(0x2004, if source == 6 { operand as u16 } else { 0x4e71 });
    memory.word(end as usize, 0x4e71);
    memory.word((end + 2) as usize, 0x7201);
    memory.word(
        (operand & !1) as usize,
        if size == 2 {
            0x8000
        } else if size == 3 {
            0x8001
        } else {
            0x8101
        },
    );
    memory.word(((operand + 2) & !1) as usize, 1);
    let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, extension], CpuClocks(17)).unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        odd: odd.is_some(),
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    let idle = u64::from(matches!(source, 2 | 4 | 8)) * 2;
    if let Some(destination_odd) = odd {
        let prefix = extensions(source)
            + if destination_odd {
                1 + usize::from(size == 2) + usize::from(destination == 4)
            } else {
                0
            };
        assert_eq!(bus.transfers, prefix);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
            if clocks == idle + prefix as u64 * 7 + 69)
        );
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
        assert_eq!(bus.transfers, count(size, source));
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == idle + count(size, source) as u64 * 7)
        );
        assert_eq!(cpu.registers.status_register(), 0x18);
        assert_eq!(cpu.registers.d, r.d);
        assert_eq!(cpu.registers.pc, end);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn memory_steps(memory: &mut MemoryBus) {
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
            for source in 0..9 {
                for destination in 2..=4 {
                    run(memory, size, source, destination, None, None);
                    checked += 1;
                    for at in 0..count(size, source) {
                        for kind in 0..3 {
                            run(memory, size, source, destination, Some((at, kind)), None);
                            checked += 1;
                        }
                    }
                    if size != 1 {
                        for operand in [false, true] {
                            run(memory, size, source, destination, None, Some(operand));
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 126_900);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
