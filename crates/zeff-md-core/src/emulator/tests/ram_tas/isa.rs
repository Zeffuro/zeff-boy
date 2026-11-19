use super::*;

#[path = "isa/ordinary.rs"]
mod ordinary;
#[path = "isa/support.rs"]
mod support;
use support::*;

struct Shape {
    mode: u16,
    register: usize,
    base: u32,
    logical: u32,
    extensions: Vec<u16>,
    begin: u64,
    clocks: u64,
}

impl Shape {
    fn new(mode: u16, register: usize, parity: u32) -> Self {
        let target = 0xffff_2040 + parity;
        let step = if register == 7 { 2 } else { 1 };
        let (base, logical, extensions, begin, clocks) = match (mode, register) {
            (2 | 3, _) => (target, target, vec![], 8, 14),
            (4, _) => (target + step, target, vec![], 10, 16),
            (5, _) => (target + 6, target, vec![0xfffa], 12, 18),
            (6, _) => (target + 18, target, vec![0x10fe], 14, 20),
            (7, 0) => (
                0,
                0xffff_8040 + parity,
                vec![0x8040 + parity as u16],
                12,
                18,
            ),
            (7, 1) => (0, target, vec![0xffff, 0x2040 + parity as u16], 16, 22),
            _ => unreachable!(),
        };
        Self {
            mode,
            register,
            base,
            logical,
            extensions,
            begin,
            clocks,
        }
    }

    fn prefix(&self, supervisor: bool) -> Vec<Event> {
        match (self.mode, self.register) {
            (2 | 3, _) => vec![],
            (4, _) => vec![Event::Idle(8, 2)],
            (5, _) | (7, 0) => vec![fetch(0x104, 8, supervisor)],
            (6, _) => vec![Event::Idle(8, 2), fetch(0x104, 10, supervisor)],
            (7, 1) => vec![fetch(0x104, 8, supervisor), fetch(0x106, 12, supervisor)],
            _ => unreachable!(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_tas_isa_a_all_42_memory_encodings_3360_inputs() {
    let mut count = 0;
    let mut encodings = 0;
    for mode in 2..=7 {
        for register in 0..if mode == 7 { 2 } else { 8 } {
            encodings += 1;
            for concurrent in [false, true] {
                for supervisor in [false, true] {
                    for parity in [0, 1] {
                        for old in OLD_BYTES {
                            for extend in [false, true] {
                                let shape = Shape::new(mode, register, parity);
                                let opcode = 0x4ac0 | mode << 3 | register as u16;
                                let mut machine =
                                    seed(opcode, &shape.extensions, concurrent, supervisor, extend);
                                machine.cpu.registers.d[1] = 0xabcd_fff0;
                                if mode != 7 {
                                    machine
                                        .cpu
                                        .registers
                                        .set_address_register(register, shape.base);
                                }
                                machine.bus.ram[shape.logical as usize & 0xffff] = old;
                                let mut expected = machine.cpu.registers;
                                expected.pc = 0x102 + shape.extensions.len() as u32 * 2;
                                expected.set_status_register(tas_status(
                                    expected.status_register(),
                                    old,
                                ));
                                if mode == 3 {
                                    expected.set_address_register(
                                        register,
                                        shape.base + if register == 7 { 2 } else { 1 },
                                    );
                                } else if mode == 4 {
                                    expected.set_address_register(register, shape.logical);
                                }
                                memory_tas(
                                    &mut machine,
                                    MemoryExpectation {
                                        opcode,
                                        logical: shape.logical,
                                        old,
                                        prefix: shape.prefix(supervisor),
                                        begin: shape.begin,
                                        clocks: shape.clocks,
                                        registers: expected,
                                    },
                                );
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(encodings, 42);
    assert_eq!(count, 3360);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_tas_isa_b_all_data_registers_320_inputs() {
    let mut count = 0;
    for concurrent in [false, true] {
        for supervisor in [false, true] {
            for register in 0..8 {
                for old in OLD_BYTES {
                    for extend in [false, true] {
                        let opcode = 0x4ac0 | register as u16;
                        let mut machine = seed(opcode, &[], concurrent, supervisor, extend);
                        machine.cpu.registers.d[register] = 0x89ab_cd00 | u32::from(old);
                        let ram = machine.bus.ram.clone();
                        let mut expected = machine.cpu.registers;
                        expected.pc = 0x102;
                        expected.d[register] = 0x89ab_cd00 | u32::from(old | 0x80);
                        expected.set_status_register(tas_status(expected.status_register(), old));
                        let (step, events) = run(&mut machine);
                        assert_eq!(step.opcode, opcode);
                        assert_eq!(step.clocks, 4);
                        assert_eq!(events, [fetch(0x104, 8, supervisor)]);
                        assert_eq!(machine.bus.ram, ram);
                        boundary(&machine, expected, 4);
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 320);
}

const ALIASES: [u32; 12] = [
    0x00e0_0000,
    0x00e0_0001,
    0x00e0_fffe,
    0x00e0_ffff,
    0x00ff_0000,
    0x00ff_0001,
    0x00ff_fffe,
    0x00ff_ffff,
    0xffff_000e,
    0xffff_000f,
    0xffff_fffe,
    0xffff_ffff,
];

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_tas_isa_c_all_logical_aliases_48_inputs() {
    let mut count = 0;
    for concurrent in [false, true] {
        for supervisor in [false, true] {
            for logical in ALIASES {
                let mut machine = seed(0x4ad0, &[], concurrent, supervisor, true);
                machine.cpu.registers.a[0] = logical;
                machine.bus.ram[logical as usize & 0xffff] = 0x7f;
                let mut expected = machine.cpu.registers;
                expected.pc = 0x102;
                expected.set_status_register(if supervisor { 0x2710 } else { 0x0710 });
                memory_tas(
                    &mut machine,
                    MemoryExpectation {
                        opcode: 0x4ad0,
                        logical,
                        old: 0x7f,
                        prefix: vec![],
                        begin: 8,
                        clocks: 14,
                        registers: expected,
                    },
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 48);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_tas_isa_c_postincrement_wrap_keeps_cached_lower_lane_2_inputs() {
    let mut count = 0;
    for concurrent in [false, true] {
        let mut machine = seed(0x4ad8, &[], concurrent, true, false);
        machine.cpu.registers.a[0] = u32::MAX;
        machine.bus.ram[0xffff] = 1;
        let mut expected = machine.cpu.registers;
        expected.pc = 0x102;
        expected.a[0] = 0;
        expected.set_status_register(0x2700);
        memory_tas(
            &mut machine,
            MemoryExpectation {
                opcode: 0x4ad8,
                logical: u32::MAX,
                old: 1,
                prefix: vec![],
                begin: 8,
                clocks: 14,
                registers: expected,
            },
        );
        count += 1;
    }
    assert_eq!(count, 2);
}
