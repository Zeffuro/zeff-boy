use super::*;
use zeff_m68k::{Registers, SUPERVISOR};

fn execute(bus: &mut MemoryBus, registers: Registers, opcode: u16, extension: &[u16]) -> usize {
    bus.word(0x2000, opcode);
    for (index, word) in extension.iter().enumerate() {
        bus.word(0x2002 + index * 2, *word);
    }
    bus.word(0x2002 + extension.len() * 2, 0x7201);
    bus.word(0x2004 + extension.len() * 2, 0x7402);
    let next = u16::from_be_bytes([bus.0[0x2002], bus.0[0x2003]]);
    let mut cpu = PrefetchCpu::from_prefetched(registers, [opcode, next], CpuClocks(0)).unwrap();
    usize::from(black_box(cpu.step_instruction(bus)).is_ok())
}

fn extensions(mode: u16, register: u16) -> &'static [u16] {
    match (mode, register) {
        (5, _) => &[0x10],
        (6, _) | (7, 3) => &[0x6000],
        (7, 0) => &[0x3010],
        (7, 1) => &[0, 0x3010],
        (7, 2) => &[0x1000],
        _ => &[],
    }
}

pub(super) fn control_steps(bus: &mut MemoryBus) {
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    registers.d[6] = 4;
    for index in 0..7 {
        registers.a[index] = 0x3008 + index as u32 * 8;
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..100 {
        for bank in [0, SUPERVISOR] {
            registers.set_status_register(bank | 31);
            registers.d[6] = 4;
            for base in [0x41c0, 0x4840, 0x4e80, 0x4ec0] {
                for mode in [2, 5, 6, 7] {
                    for register in 0..if mode == 7 { 4 } else { 8 } {
                        for destination in 0..if base == 0x41c0 { 8 } else { 1 } {
                            executed += execute(
                                bus,
                                registers,
                                base | destination << 9 | mode << 3 | register,
                                extensions(mode, register),
                            );
                        }
                    }
                }
            }
            for condition in 0..16 {
                for mode in 0..8 {
                    if mode == 1 {
                        continue;
                    }
                    for register in 0..if mode == 7 { 2 } else { 8 } {
                        executed += execute(
                            bus,
                            registers,
                            0x50c0 | condition << 8 | mode << 3 | register,
                            extensions(mode, register),
                        );
                    }
                }
                for register in 0..8 {
                    for counter in [0, 1, 0xffff] {
                        registers.d[usize::from(register)] = counter;
                        executed +=
                            execute(bus, registers, 0x50c8 | condition << 8 | register, &[0x100]);
                    }
                }
            }
            for opcode in [0x6102, 0x6104, 0x617e, 0x6180] {
                executed += execute(bus, registers, opcode, &[]);
            }
            for displacement in [0, 0x100, 0xff80] {
                executed += execute(bus, registers, 0x6100, &[displacement]);
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 299_800);
    assert_eq!(after - before, 0);
}
