use super::physical_move_updates::Device;
use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

const FIELDS: [u16; 9] = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b];

fn extensions(source: usize) -> usize {
    if source == 6 {
        2
    } else {
        usize::from(source >= 3)
    }
}

fn run(
    memory: &mut MemoryBus,
    form: (u16, usize, usize, usize),
    supervisor: bool,
    failure: Option<(usize, u8)>,
    odd: bool,
) {
    let (size, source, register, destination) = form;
    let opcode = 0xb000
        | (destination as u16) << 9
        | size << 6
        | FIELDS[source]
        | if source < 5 { register as u16 } else { 0 };
    let stride = if size == 2 { 4 } else { 2 };
    let operand = 0x3000 + u32::from(odd);
    let end = 0x2002 + extensions(source) as u32 * 2;
    let count = extensions(source) + 2 + usize::from(size == 2);
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.d.fill(1);
    r.d[5] = 0x1000;
    r.set_status_register(0x001f | if supervisor { 0x2000 } else { 0 });
    if source < 5 {
        r.set_address_register(
            register,
            if source == 4 {
                0x2000
            } else {
                operand + if source == 2 { stride } else { 0 }
            },
        );
    }
    let extension = match source {
        0..=2 => 0x4e71,
        3 | 6 => 0,
        4 => 0x5800 | u16::from(odd),
        5 => operand as u16,
        7 => (operand - 0x2002) as u16,
        8 => 0x58fe + u16::from(odd),
        _ => unreachable!(),
    };
    memory.word(0x2004, if source == 6 { operand as u16 } else { 0x4e71 });
    memory.word(end as usize, 0x4e71);
    memory.word((end + 2) as usize, 0x7201);
    memory.word((operand & !1) as usize, 0x8000);
    memory.word(((operand + 2) & !1) as usize, 1);
    let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, extension], CpuClocks(17)).unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        odd,
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    let idle = u64::from(matches!(source, 2 | 4 | 8)) * 2;
    if odd {
        assert_eq!(bus.transfers, extensions(source));
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
            if clocks == idle + extensions(source) as u64 * 7 + 69)
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
        assert_eq!(bus.transfers, count);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == idle + count as u64 * 7 + u64::from(size == 2) * 2)
        );
        let mut expected = r;
        expected.pc = end;
        expected.set_status_register((r.status_register() & !15) | 11);
        if source == 1 || source == 2 {
            let address = r.address_register(register).unwrap();
            expected.set_address_register(
                register,
                if source == 1 {
                    address + stride
                } else {
                    address - stride
                },
            );
        }
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn cmp_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for supervisor in [false, true] {
            for size in [1, 2] {
                for source in 0..9 {
                    let count = extensions(source) + 2 + usize::from(size == 2);
                    for register in 0..if source < 5 { 8 } else { 1 } {
                        for destination in 0..8 {
                            let form = (size, source, register, destination);
                            run(memory, form, supervisor, None, false);
                            checked += 1;
                            for at in 0..count {
                                for kind in 0..3 {
                                    run(memory, form, supervisor, Some((at, kind)), false);
                                    checked += 1;
                                }
                            }
                        }
                    }
                    run(memory, (size, source, 0, 0), supervisor, None, true);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 1_402_000);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
