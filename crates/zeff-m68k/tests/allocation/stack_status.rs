use super::*;
use zeff_m68k::{Registers, SUPERVISOR};

fn word(bus: &mut MemoryBus, address: u32, value: u16) {
    bus.word(address as usize, value);
}

fn long(bus: &mut MemoryBus, address: u32, value: u32) {
    word(bus, address, (value >> 16) as u16);
    word(bus, address + 2, value as u16);
}

fn execute(bus: &mut MemoryBus, registers: Registers, opcode: u16, extension: &[u16]) -> usize {
    word(bus, registers.pc, opcode);
    for (index, value) in extension.iter().enumerate() {
        word(bus, registers.pc + 2 + index as u32 * 2, *value);
    }
    word(bus, registers.pc + 2 + extension.len() as u32 * 2, 0x7201);
    word(bus, registers.pc + 4 + extension.len() as u32 * 2, 0x7402);
    let next = u16::from_be_bytes([bus.0[0x2002], bus.0[0x2003]]);
    let mut cpu = PrefetchCpu::from_prefetched(registers, [opcode, next], CpuClocks(0)).unwrap();
    usize::from(black_box(cpu.step_instruction(bus)).is_ok())
}

fn operand(
    bus: &mut MemoryBus,
    registers: Registers,
    mode: u16,
    reg: u16,
    value: u16,
) -> &'static [u16] {
    let base = registers.address_register(usize::from(reg)).unwrap();
    let (address, extension): (Option<u32>, &[u16]) = match (mode, reg) {
        (0, _) => (None, &[]),
        (2 | 3, _) => (Some(base), &[]),
        (4, _) => (Some(base - 2), &[]),
        (5, _) => (Some(base + 16), &[16]),
        (6, _) => (Some(base + 4), &[0x6000]),
        (7, 0) => (Some(0x3010), &[0x3010]),
        (7, 1) => (Some(0x3010), &[0, 0x3010]),
        (7, 2) => (Some(0x3002), &[0x1000]),
        (7, 3) => (Some(0x2082), &[0x607c]),
        (7, 4) => {
            return match value {
                0 => &[0],
                0x2000 => &[0x2000],
                0x8000 => &[0x8000],
                _ => &[0xffff],
            };
        }
        _ => unreachable!(),
    };
    if let Some(address) = address {
        word(bus, address, value);
    }
    extension
}

pub(super) fn stack_status_steps(bus: &mut MemoryBus) {
    for vector in [7, 8].into_iter().chain(32..48) {
        long(bus, vector * 4, 0x1000);
    }
    word(bus, 0x1000, 0x4e71);
    word(bus, 0x1002, 0x7201);
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.usp = 0x6000;
    registers.ssp = 0x5000;
    for index in 0..7 {
        registers.a[index] = 0x3008 + index as u32 * 8;
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..100 {
        for bank in [0, SUPERVISOR] {
            registers.set_status_register(bank | 31);
            for reg in 0..8 {
                for displacement in [0, 2, 126, 256, 0xff80] {
                    executed += execute(bus, registers, 0x4e50 | reg, &[displacement]);
                }
                for pointer in [0x3000, 0x3002, 0x5000, 0x6000] {
                    long(
                        bus,
                        registers.address_register(usize::from(reg)).unwrap(),
                        pointer,
                    );
                    executed += execute(bus, registers, 0x4e58 | reg, &[]);
                }
                for direction in [0, 8] {
                    executed += execute(bus, registers, 0x4e60 | direction | reg, &[]);
                }
            }
            for ccr in [0, 31, 0xffe0, 0xffff] {
                for target in [0x1000, 0x1002, 0x2000, 0x2002] {
                    word(bus, registers.stack_pointer(), ccr);
                    long(bus, registers.stack_pointer() + 2, target);
                    executed += execute(bus, registers, 0x4e77, &[]);
                }
            }
            for overflow in [0, 2] {
                registers.set_status_register(bank | 29 | overflow);
                executed += execute(bus, registers, 0x4e76, &[]);
            }
            registers.set_status_register(bank | 31);
            for vector in 0..16 {
                executed += execute(bus, registers, 0x4e40 | vector, &[]);
            }
            for value in [0, 0x2000, 0x8000, 0xffff] {
                registers.d.fill(u32::from(value));
                registers.d[6] = 4;
                for base in [0x40c0, 0x44c0, 0x46c0] {
                    for mode in [0, 2, 3, 4, 5, 6, 7] {
                        for reg in 0..if mode == 7 {
                            if base == 0x40c0 { 2 } else { 5 }
                        } else {
                            8
                        } {
                            let extension = operand(bus, registers, mode, reg, value);
                            executed += execute(bus, registers, base | mode << 3 | reg, extension);
                        }
                    }
                }
            }
        }
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(executed, 149_200);
    assert_eq!(after - before, 0);
}
