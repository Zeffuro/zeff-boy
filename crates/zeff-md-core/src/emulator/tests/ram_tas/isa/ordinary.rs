use super::*;

#[derive(Clone, Copy)]
enum Schedule {
    WriteFetch,
    ImmediateReadFetchWrite,
    ReadFetchWrite,
}

struct Ordinary {
    opcode: u16,
    extensions: &'static [u16],
    old: u16,
    result: u16,
    before: u16,
    after: u16,
    word: bool,
    schedule: Schedule,
}

const ROWS: [Ordinary; 10] = [
    Ordinary {
        opcode: 0x1080,
        extensions: &[],
        old: 0x35,
        result: 0x80,
        before: 0x271f,
        after: 0x2718,
        word: false,
        schedule: Schedule::WriteFetch,
    },
    Ordinary {
        opcode: 0x08d0,
        extensions: &[0],
        old: 0x20,
        result: 0x21,
        before: 0x271b,
        after: 0x271f,
        word: false,
        schedule: Schedule::ImmediateReadFetchWrite,
    },
    Ordinary {
        opcode: 0x0850,
        extensions: &[0],
        old: 0x81,
        result: 0x80,
        before: 0x271f,
        after: 0x271b,
        word: false,
        schedule: Schedule::ImmediateReadFetchWrite,
    },
    Ordinary {
        opcode: 0x0890,
        extensions: &[7],
        old: 0xff,
        result: 0x7f,
        before: 0x271f,
        after: 0x271b,
        word: false,
        schedule: Schedule::ImmediateReadFetchWrite,
    },
    Ordinary {
        opcode: 0x4210,
        extensions: &[],
        old: 0xff,
        result: 0,
        before: 0x271f,
        after: 0x2714,
        word: false,
        schedule: Schedule::ReadFetchWrite,
    },
    Ordinary {
        opcode: 0x4610,
        extensions: &[],
        old: 0x7f,
        result: 0x80,
        before: 0x271f,
        after: 0x2718,
        word: false,
        schedule: Schedule::ReadFetchWrite,
    },
    Ordinary {
        opcode: 0x4410,
        extensions: &[],
        old: 1,
        result: 0xff,
        before: 0x271f,
        after: 0x2719,
        word: false,
        schedule: Schedule::ReadFetchWrite,
    },
    Ordinary {
        opcode: 0x5210,
        extensions: &[],
        old: 0x7f,
        result: 0x80,
        before: 0x271f,
        after: 0x270a,
        word: false,
        schedule: Schedule::ReadFetchWrite,
    },
    Ordinary {
        opcode: 0xe1d0,
        extensions: &[],
        old: 0x4001,
        result: 0x8002,
        before: 0x271f,
        after: 0x270a,
        word: true,
        schedule: Schedule::ReadFetchWrite,
    },
    Ordinary {
        opcode: 0x4810,
        extensions: &[],
        old: 1,
        result: 0x98,
        before: 0x271f,
        after: 0x2719,
        word: false,
        schedule: Schedule::ReadFetchWrite,
    },
];

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_tas_isa_d_ten_ordinary_write_families_40_inputs() {
    let mut count = 0;
    for concurrent in [false, true] {
        for row in &ROWS {
            for location in [0, 1] {
                let logical = if row.word {
                    [0xffff_2040, 0xffff_20fe][location]
                } else {
                    0xffff_2040 + location as u32
                };
                let mut machine = seed(row.opcode, row.extensions, concurrent, true, true);
                machine.cpu.registers.a[0] = logical;
                machine.cpu.registers.d[0] = 0x1234_5680;
                machine.cpu.registers.set_status_register(row.before);
                let offset = logical as usize & 0xffff;
                if row.word {
                    machine.bus.ram[offset..offset + 2].copy_from_slice(&row.old.to_be_bytes());
                } else {
                    machine.bus.ram[offset] = row.old as u8;
                }
                let mut ram = machine.bus.ram.clone();
                if row.word {
                    ram[offset..offset + 2].copy_from_slice(&row.result.to_be_bytes());
                } else {
                    ram[offset] = row.result as u8;
                }
                let mut expected = machine.cpu.registers;
                expected.pc = 0x102 + row.extensions.len() as u32 * 2;
                expected.set_status_register(row.after);
                let data = access(logical, true, row.word);
                let value = packed(logical, row.result, row.word);
                let (clocks, events) = match row.schedule {
                    Schedule::WriteFetch => (
                        8,
                        vec![Event::Write(data, value, 8, 0), fetch(0x104, 12, true)],
                    ),
                    Schedule::ImmediateReadFetchWrite => (
                        16,
                        vec![
                            fetch(0x104, 8, true),
                            Event::Read(data, 12, 0),
                            fetch(0x106, 16, true),
                            Event::Write(data, value, 20, 0),
                        ],
                    ),
                    Schedule::ReadFetchWrite => (
                        12,
                        vec![
                            Event::Read(data, 8, 0),
                            fetch(0x104, 12, true),
                            Event::Write(data, value, 16, 0),
                        ],
                    ),
                };
                let (step, actual) = run(&mut machine);
                assert_eq!(step.opcode, row.opcode);
                assert_eq!(step.clocks, clocks);
                assert_eq!(actual, events);
                assert_eq!(machine.bus.ram, ram);
                boundary(&machine, expected, clocks);
                count += 1;
            }
        }
    }
    assert_eq!(count, 40);
}
