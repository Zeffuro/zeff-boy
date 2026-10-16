use super::*;
use zeff_m68k::{Registers, SUPERVISOR};

fn execute(bus: &mut MemoryBus, registers: Registers, opcode: u16, extension: &[u16]) -> usize {
    bus.word(0x2000, opcode);
    for (index, value) in extension.iter().enumerate() {
        bus.word(0x2002 + index * 2, *value);
    }
    bus.word(0x2002 + extension.len() * 2, 0x7201);
    bus.word(0x2004 + extension.len() * 2, 0x7402);
    let mut cpu =
        PrefetchCpu::from_prefetched(registers, [opcode, extension[0]], CpuClocks(0)).unwrap();
    usize::from(black_box(cpu.step_instruction(bus)).is_ok())
}

fn movem_extension(mode: u16, reg: u16, mask: u16) -> ([u16; 3], usize) {
    match (mode, reg) {
        (2..=4, _) => ([mask, 0, 0], 1),
        (5, _) => ([mask, 16, 0], 2),
        (6, _) => ([mask, 0x7000, 0], 2),
        (7, 0) => ([mask, 0x3010, 0], 2),
        (7, 1) => ([mask, 0, 0x3010], 3),
        (7, 2) => ([mask, 0x1000, 0], 2),
        (7, 3) => ([mask, 0x707c, 0], 2),
        _ => unreachable!(),
    }
}

pub(super) fn transfer_steps(bus: &mut MemoryBus) {
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    for index in 0..7 {
        registers.a[index] = 0x3008 + index as u32 * 8;
    }
    registers.d.fill(0x89ab_cdef);
    registers.d[7] = 4;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..100 {
        for bank in [0, SUPERVISOR] {
            registers.set_status_register(bank | 31);
            for load in [false, true] {
                for long in [false, true] {
                    let modes = if load {
                        [2, 3, 5, 6, 7]
                    } else {
                        [2, 4, 5, 6, 7]
                    };
                    for mode in modes {
                        for reg in 0..if mode == 7 {
                            if load { 4 } else { 2 }
                        } else {
                            8
                        } {
                            for mask in [0, 0xffff, 0x5555, 0xaaaa]
                                .into_iter()
                                .chain((0..16).map(|bit| 1 << bit))
                            {
                                let (words, length) = movem_extension(mode, reg, mask);
                                let opcode = 0x4880
                                    | if load { 0x0400 } else { 0 }
                                    | if long { 0x0040 } else { 0 }
                                    | mode << 3
                                    | reg;
                                executed += execute(bus, registers, opcode, &words[..length]);
                            }
                        }
                    }
                }
            }
            for destination in 0..8 {
                for reg in 0..8 {
                    for long in [false, true] {
                        for store in [false, true] {
                            for odd in [0, 1] {
                                for displacement in [0xff80_u16, 0xfffe, 0, 16, 127] {
                                    let mut case = registers;
                                    case.set_address_register(
                                        usize::from(reg),
                                        case.address_register(usize::from(reg)).unwrap() + odd,
                                    );
                                    case.d[usize::from(destination)] = 0x89ab_cdef;
                                    let opcode = 0x0108
                                        | destination << 9
                                        | reg
                                        | if long { 0x0040 } else { 0 }
                                        | if store { 0x0080 } else { 0 };
                                    executed += execute(bus, case, opcode, &[displacement]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 1_072_000);
    assert_eq!(after - before, 0);
}
