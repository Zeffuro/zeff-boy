use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod known;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TestPorts {
    values: [u16; 8],
    events: Vec<(bool, u8, u16, bool, u16)>,
    inputs: Option<u16>,
    fail: bool,
}

impl Ports for TestPorts {
    fn read(&mut self, index: u8, st: u16, blind: bool) -> Result<u16, Fault> {
        let value = self.values[usize::from(index)];
        self.events.push((false, index, st, blind, value));
        self.values[usize::from(index)] = value.wrapping_add(1);
        if self.fail {
            return Err(Fault::Port {
                index,
                reason: "fixture read",
            });
        }
        Ok(value)
    }

    fn write(&mut self, index: u8, st: u16, blind: bool, value: u16) -> Result<(), Fault> {
        self.events.push((true, index, st, blind, value));
        self.values[usize::from(index)] = value;
        if self.fail {
            return Err(Fault::Port {
                index,
                reason: "fixture write",
            });
        }
        Ok(())
    }

    fn status_inputs(&mut self) -> Result<u16, Fault> {
        self.inputs.ok_or(Fault::StatusInputs)
    }
}

fn run(core: &mut Core, ports: &mut TestPorts, words: &[u16]) -> Result<Step, Fault> {
    core.step(
        |address| {
            words
                .get(usize::from(address))
                .copied()
                .ok_or(Fault::ProgramRead(address))
        },
        ports,
    )
}

fn one(core: &mut Core, ports: &mut TestPorts, words: &[u16]) -> Step {
    core.pc = 0;
    run(core, ports, words).unwrap()
}

fn synthetic_program() {
    let words = [
        0x0810, 2, 0x0820, 3, 0x0037, 0x4800, 12, 0x0840, 0, 0x00b3, 0x4c00, 10, 0x8800, 1, 0x0065,
    ];
    let expected = [
        (0, 2, 0, 2),
        (2, 4, 0, 2),
        (4, 5, 12, 1),
        (5, 12, 12, 2),
        (12, 14, 0x0001000c, 2),
        (14, 7, 0x0001000c, 1),
        (7, 9, 0x0001000c, 2),
        (9, 10, 0x0001000c, 1),
        (10, 10, 0x0001000c, 2),
    ];
    let mut cpu = Core::synthetic(0);
    let mut ports = TestPorts::default();
    for (pc, next, a, reads) in expected {
        let step = run(&mut cpu, &mut ports, &words).unwrap();
        assert_eq!(
            (step.pc, cpu.pc, cpu.a, step.program_reads),
            (pc, next, a, reads)
        );
    }
    assert_eq!(cpu.stack_len, 0);
    assert_eq!(ports.events, [(true, 3, 0, false, 1)]);
}

fn transfer_forms() {
    let mut cpu = Core::synthetic(0);
    let mut ports = TestPorts::default();
    cpu.a = 0x12345678;
    one(&mut cpu, &mut ports, &[0x0013]);
    assert_eq!(cpu.x, 0x1234);
    one(&mut cpu, &mut ports, &[0x0830, 0xabcd]);
    assert_eq!(cpu.a, 0xabcd5678);
    cpu.ram[0][0x20] = 0x2468;
    cpu.pointers[0] = 0x20;
    one(&mut cpu, &mut ports, &[0x0220]);
    assert_eq!(cpu.y, 0x2468);
    one(&mut cpu, &mut ports, &[0x0424]);
    assert_eq!((cpu.ram[0][0x20], cpu.pointers[0]), (0x2468, 0x21));
    cpu.ram[1][0x80] = 0x8765;
    one(&mut cpu, &mut ports, &[0x0780]);
    assert_eq!(cpu.a, 0x87655678);
    one(&mut cpu, &mut ports, &[0x0f81]);
    assert_eq!(cpu.ram[1][0x81], 0x8765);
    one(&mut cpu, &mut ports, &[0x0d07, 0xbeef]);
    assert_eq!(cpu.ram[1][1], 0xbeef);
    one(&mut cpu, &mut ports, &[0x1cfe]);
    assert_eq!(cpu.pointers[4], 0xfe);
    one(&mut cpu, &mut ports, &[0x1320]);
    assert_eq!(cpu.y, 0xfe);
    one(&mut cpu, &mut ports, &[0x1520]);
    assert_eq!(cpu.pointers[4], 0xfe);
    cpu.ram[0][0x21] = 3;
    let step = one(&mut cpu, &mut ports, &[0x0a10, 0, 0, 0x5555]);
    assert_eq!(
        (
            cpu.x,
            cpu.ram[0][0x21],
            step.instruction_words,
            step.program_reads
        ),
        (0x5555, 4, 1, 2)
    );
    cpu.a = 0x00025678;
    one(&mut cpu, &mut ports, &[0x4a20, 0, 0x1357]);
    assert_eq!(cpu.y, 0x1357);
    cpu.x = 0xfffe;
    cpu.y = 3;
    cpu.p = 0xfffffff4;
    one(&mut cpu, &mut ports, &[0x0037]);
    assert_eq!(cpu.a, 0xfffffff4);
    one(&mut cpu, &mut ports, &[0x00f1]);
    assert_eq!(cpu.a, 0xfffffffe);
    one(&mut cpu, &mut ports, &[0x001f]);
    assert_eq!(cpu.x, 0xfffe);
    ports.values[6] = 0x4321;
    one(&mut cpu, &mut ports, &[0x000e]);
    one(&mut cpu, &mut ports, &[0x00e0]);
    one(&mut cpu, &mut ports, &[0x000f]);
    one(&mut cpu, &mut ports, &[0x00f0]);
    assert_eq!(
        ports.events,
        [
            (false, 6, 0, true, 0x4321),
            (true, 6, 0, true, 0xffff),
            (false, 7, 0, true, 0),
            (true, 7, 0, true, 0xffff),
        ]
    );
    assert_eq!(cpu.a, 0xfffffffe);
}

fn arithmetic_forms() {
    let addressing = [0x0001, 0x0200, 0x0608, 0x0800, 0x0a00, 0x1200, 0x1802];
    let operations = [
        (0x2000, 0x00010001, 0, false),
        (0x6000, 0x00030001, 0, true),
        (0x8000, 0x00050001, 0, false),
        (0xa000, 0x00020000, 0, false),
        (0xc000, 0x00030001, 0, false),
        (0xe000, 0x00010001, 0, false),
    ];
    for (operation, expected, flags, compare) in operations {
        for address in addressing {
            let mut cpu = Core::synthetic(0);
            let mut ports = TestPorts::default();
            cpu.a = 0x00030001;
            cpu.x = 2;
            cpu.pointers[0] = 2;
            cpu.ram[0][2] = if address == 0x0a00 { 3 } else { 2 };
            cpu.ram[0][8] = 2;
            one(&mut cpu, &mut ports, &[operation | address, 2, 0, 2]);
            assert_eq!((cpu.a, cpu.st & NZ, cpu.flags_known), (expected, flags, NZ));
            assert_eq!(cpu.a == 0x00030001, compare || operation == 0xc000);
        }
    }
    for (opcode, a, operand, expected, flags) in [
        (0x8800, 0x7fff0000, 1, 0x80000000, FLAG_N),
        (0x8800, 0xffff0000, 1, 0, FLAG_Z),
        (0x2800, 0, 1, 0xffff0000, FLAG_N),
        (0xa800, 0x12345678, 0, 0, FLAG_Z),
        (0x6800, 0x10001, 1, 0x10001, 0),
    ] {
        let mut cpu = Core::synthetic(0);
        cpu.a = a;
        one(&mut cpu, &mut TestPorts::default(), &[opcode, operand]);
        assert_eq!((cpu.a, cpu.st & NZ), (expected, flags));
    }
    for (opcode, a, expected, flags) in [
        (0x9002, 0x80000001, 0xc0000000, FLAG_N),
        (0x9003, 0x80000000, 0, FLAG_Z),
        (0x9006, 1, 0xffffffff, FLAG_N),
        (0x9007, 0x80000000, 0x80000000, FLAG_N),
        (0x9007, 0xffffffff, 1, 0),
    ] {
        let mut cpu = Core::synthetic(0);
        cpu.a = a;
        one(&mut cpu, &mut TestPorts::default(), &[opcode]);
        assert_eq!((cpu.a, cpu.st & NZ), (expected, flags));
    }
}

fn pointers_and_products() {
    for (st, pointer, opcode, expected) in [
        (0, 0xff, 0x0214, 0),
        (0, 0, 0x0218, 0xff),
        (3, 0x2f, 0x021c, 0x28),
        (3, 0x28, 0x0218, 0x2f),
        (3, 0x2f, 0x0214, 0x30),
        (7, 0xff, 0x021c, 0x80),
    ] {
        let mut cpu = Core::synthetic(0);
        cpu.st = st;
        cpu.pointers[0] = pointer;
        cpu.ram[0][usize::from(pointer)] = 0x9876;
        one(&mut cpu, &mut TestPorts::default(), &[opcode]);
        assert_eq!((cpu.x, cpu.pointers[0]), (0x9876, expected));
    }
    for (opcode, expected_a) in [(0xb700, 0), (0x9700, 88), (0x3700, 112)] {
        let mut cpu = Core::synthetic(0);
        cpu.a = 100;
        cpu.x = 0xfffe;
        cpu.y = 3;
        cpu.p = 0xfffffff4;
        cpu.ram[0][0] = 4;
        cpu.ram[1][0] = 0xfffb;
        one(&mut cpu, &mut TestPorts::default(), &[opcode]);
        assert_eq!(
            (cpu.a, cpu.x, cpu.y, cpu.p),
            (expected_a, 4, 0xfffb, 0xffffffd8)
        );
    }
    let mut cpu = Core::synthetic(0);
    cpu.ram[0][0] = 0x8000;
    cpu.ram[1][0] = 0x8000;
    one(&mut cpu, &mut TestPorts::default(), &[0xb700]);
    assert_eq!(cpu.p, 0x80000000);
}

fn conditions_and_faults() {
    for (opcode, st, pins, next) in [
        (0x4c00, 0, 0, 7),
        (0x4c50, 0, 0, 7),
        (0x4d50, 0, 0, 2),
        (0x4d50, FLAG_Z, 0, 7),
        (0x4d70, FLAG_N, 0, 7),
        (0x4d40, FLAG_L, 0, 7),
        (0x4d60, FLAG_OV, 0, 7),
        (0x4d20, 0, 0x0400, 7),
        (0x4d30, 0, 0x0800, 7),
    ] {
        let mut cpu = Core::synthetic(0);
        cpu.st = st;
        let mut ports = TestPorts {
            inputs: Some(pins),
            ..Default::default()
        };
        one(&mut cpu, &mut ports, &[opcode, 7]);
        assert_eq!(cpu.pc, next);
    }
    let mut cpu = Core::synthetic(0);
    let mut ports = TestPorts {
        inputs: Some(0x0400),
        ..Default::default()
    };
    one(&mut cpu, &mut ports, &[0x0014]);
    assert_eq!(cpu.x, 0x0400);
    for _ in 0..6 {
        one(&mut cpu, &mut ports, &[0x0850, 0xabcd]);
    }
    assert_eq!(cpu.stack_len, 6);
    atomic_fault(&cpu, &ports, &[0x0850, 1], Fault::StackOverflow);
    for _ in 0..6 {
        one(&mut cpu, &mut ports, &[0x0015]);
    }
    assert_eq!((cpu.stack_len, cpu.x), (0, 0xabcd));
    atomic_fault(&cpu, &ports, &[0x0015], Fault::StackUnderflow);
    for opcode in [0xffff, 0x0811, 0x4c10, 0x4d00, 0x9000, 0x9600] {
        atomic_fault(&cpu, &ports, &[opcode], Fault::Opcode(opcode));
    }
    for st in [0x0008, 0x0010, 0x0080, 0x0100, 0x0200, 0x0400, 0x0800] {
        atomic_fault(&cpu, &ports, &[0x0840, st], Fault::StatusMode(st));
    }
    atomic_fault(&cpu, &ports, &[0x0016], Fault::RegisterRead(6));
    atomic_fault(&cpu, &ports, &[0x0071], Fault::RegisterWrite(7));
    atomic_fault(&cpu, &ports, &[0x0810], Fault::ProgramRead(1));
    cpu.st = 3;
    atomic_fault(
        &cpu,
        &ports,
        &[0x041c],
        Fault::PointerMode {
            index: 0,
            modifier: 3,
        },
    );
    atomic_fault(
        &cpu,
        &ports,
        &[0x0a14],
        Fault::PointerMode {
            index: 0,
            modifier: 1,
        },
    );
    atomic_fault(
        &cpu,
        &ports,
        &[0x1b00],
        Fault::PointerMode {
            index: 3,
            modifier: 0,
        },
    );
    cpu.st = 0;
    ports.fail = true;
    atomic_fault(
        &cpu,
        &ports,
        &[0x0081],
        Fault::Port {
            index: 0,
            reason: "fixture write",
        },
    );
    atomic_fault(
        &cpu,
        &ports,
        &[0x0018],
        Fault::Port {
            index: 0,
            reason: "fixture read",
        },
    );
    ports.fail = false;
    ports.inputs = None;
    atomic_fault(&cpu, &ports, &[0x0014], Fault::StatusInputs);
    one(&mut cpu, &mut ports, &[0x8800, 1]);
    atomic_fault(
        &cpu,
        &ports,
        &[0x0014],
        Fault::UnknownFlags(FLAG_L | FLAG_OV),
    );
    atomic_fault(&cpu, &ports, &[0x4c40, 2], Fault::UnknownFlags(FLAG_L));
    atomic_fault(&cpu, &ports, &[0x4c60, 2], Fault::UnknownFlags(FLAG_OV));
}

fn atomic_fault(cpu: &Core, ports: &TestPorts, words: &[u16], expected: Fault) {
    let mut candidate = cpu.clone();
    candidate.pc = 0;
    let baseline = candidate.clone();
    let mut candidate_ports = ports.clone();
    assert_eq!(
        run(&mut candidate, &mut candidate_ports, words),
        Err(expected)
    );
    assert_eq!(candidate, baseline);
    assert_eq!(&candidate_ports, ports);
}

fn states() {
    let mut cpu = Core::synthetic(0);
    let mut ports = TestPorts::default();
    cpu.ram[0][0] = 2;
    cpu.ram[1][0] = 3;
    one(&mut cpu, &mut ports, &[0xb700]);
    cpu.stack_len = 2;
    cpu.stack = [1, 2, 3, 4, 5, 6];
    cpu.pointers = [1, 2, 3, 0, 4, 5, 6, 0];
    cpu.ram[1][255] = 0xbeef;
    let mut writer = StateWriter::new();
    cpu.write_state(&mut writer).unwrap();
    let bytes = writer.into_bytes();
    assert_eq!(bytes.len(), 1063);
    let mut reader = StateReader::new(&bytes);
    let mut restored = Core::read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(restored, cpu);
    one(&mut cpu, &mut ports, &[0x8007]);
    one(&mut restored, &mut TestPorts::default(), &[0x8007]);
    assert_eq!(restored, cpu);
    for (offset, value) in [(8, 0xff), (14, 8), (17, 0), (18, 7), (34, 1)] {
        let mut malformed = bytes.clone();
        malformed[offset] = value;
        assert!(Core::read_state(&mut StateReader::new(&malformed)).is_err());
    }
    for size in [0, 18, 100, 1062] {
        assert!(Core::read_state(&mut StateReader::new(&bytes[..size])).is_err());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn scalar_cpu_semantic_contract() {
    synthetic_program();
    transfer_forms();
    arithmetic_forms();
    pointers_and_products();
    conditions_and_faults();
    states();
    known::accumulator_provenance();
}
