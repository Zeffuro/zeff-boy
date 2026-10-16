use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;

mod coverage;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bus {
    memory: Vec<u8>,
    events: RefCell<Vec<(u8, u16, u8)>>,
    io_cycles: Vec<IoWriteCycle>,
    irq: bool,
    nmi: bool,
    acknowledge: u8,
}

impl Bus {
    fn new(program: &[u8]) -> Self {
        let mut memory = vec![0x5a; 65536];
        memory[..program.len()].copy_from_slice(program);
        Self {
            memory,
            events: RefCell::default(),
            io_cycles: Vec::new(),
            irq: false,
            nmi: false,
            acknowledge: 0xff,
        }
    }
}

impl Z80Bus for Bus {
    fn cpu_read(&self, address: u16) -> u8 {
        let value = self.memory[usize::from(address)];
        self.events.borrow_mut().push((0, address, value));
        value
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.events.borrow_mut().push((1, address, value));
        self.memory[usize::from(address)] = value;
    }
    fn io_read(&mut self, port: u8) -> u8 {
        self.events.borrow_mut().push((2, u16::from(port), 0xa5));
        0xa5
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.events.borrow_mut().push((3, u16::from(port), value));
    }
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.io_cycles.push(cycle);
        self.io_write(cycle.port, cycle.value);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        let pending = self.nmi;
        self.nmi = false;
        self.events.borrow_mut().push((4, 0, 0));
        pending
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.irq = false;
        self.events.borrow_mut().push((5, 0, self.acknowledge));
        self.acknowledge
    }
    fn read_interrupt_instruction(&mut self, pc: u16) -> u8 {
        self.events.borrow_mut().push((6, pc, 0x42));
        0x42
    }
}

fn encode(cpu: &Cpu, bus: &BusState) -> Vec<u8> {
    let mut w = Writer::new();
    cpu.write_continuation(bus, &mut w);
    w.into_bytes()
}

fn restore(bytes: &[u8]) -> (Cpu, BusState) {
    let mut cpu = Cpu::new_with_reset(ResetState::new(0xdead, 0xbeef));
    let mut bus = BusState::default();
    let mut r = Reader::new(bytes);
    cpu.read_continuation(&mut bus, &mut r).unwrap();
    assert!(r.is_exhausted());
    (cpu, bus)
}

fn witness(cpu: &Cpu, bus: &BusState, memory: &Bus) {
    let (mut restored, mut restored_bus) = restore(&encode(cpu, bus));
    assert_eq!(restored, *cpu);
    assert_eq!(restored_bus, *bus);
    let mut expected = cpu.clone();
    let mut expected_bus = *bus;
    let mut expected_memory = memory.clone();
    let mut restored_memory = memory.clone();
    for _ in 0..16 {
        let next = expected.step_bus_segment(&mut expected_memory, &mut expected_bus);
        assert_eq!(
            restored.step_bus_segment(&mut restored_memory, &mut restored_bus),
            next
        );
        assert_eq!(restored, expected);
        assert_eq!(restored_bus, expected_bus);
        assert_eq!(restored_memory, expected_memory);
        if next.is_none_or(|step| step.complete) {
            return;
        }
    }
    panic!("synthetic instruction did not finish");
}

fn check_instruction(program: &[u8], setup: impl FnOnce(&mut Cpu, &mut Bus)) -> BTreeSet<String> {
    let mut cpu = Cpu::new_with_reset(ResetState::new(0, 0x8000));
    cpu.regs = Registers {
        b: 2,
        c: 3,
        d: 0x50,
        e: 0,
        h: 0x40,
        l: 0,
        ix: 0x4100,
        iy: 0x4200,
        sp: 0x8000,
        ..cpu.regs
    };
    let mut memory = Bus::new(program);
    let mut bus = BusState::default();
    let mut coverage = BTreeSet::new();
    setup(&mut cpu, &mut memory);
    for _ in 0..16 {
        coverage::record(&bus, &mut coverage);
        witness(&cpu, &bus, &memory);
        let step = cpu.step_bus_segment(&mut memory, &mut bus).unwrap();
        if step.complete {
            witness(&cpu, &bus, &memory);
            return coverage;
        }
    }
    panic!("synthetic instruction did not finish: {program:02x?}");
}

#[test]
fn immediate_load_roundtrip_continues_the_pending_high_byte() {
    check_instruction(&[0x21, 0x34, 0x12], |_, _| {});
}

#[test]
fn load_and_stack_phase_families_continue_exactly() {
    let mut visited = BTreeSet::new();
    for program in [
        &[0x06, 0x35][..],
        &[0x01, 0x34, 0x12],
        &[0xdd, 0x21, 0x34, 0x12],
        &[0xfd, 0x2a, 0x00, 0x60],
        &[0x22, 0x00, 0x60],
        &[0x2a, 0x00, 0x60],
        &[0x32, 0x00, 0x60],
        &[0x3a, 0x00, 0x60],
        &[0x36, 0x37],
        &[0xc3, 0x10, 0x00],
        &[0xca, 0x10, 0x00],
        &[0xcd, 0x10, 0x00],
        &[0xcc, 0x10, 0x00],
        &[0xc8],
        &[0xc9],
        &[0xc5],
        &[0xc1],
        &[0xe3],
        &[0xff],
        &[0xdd, 0xe3],
        &[0xfd, 0xe1],
        &[0xed, 0x45],
    ] {
        visited.extend(check_instruction(program, |_, _| {}));
    }
    for opcode in [0xca, 0xcc, 0xc8] {
        visited.extend(check_instruction(&[opcode, 0x10, 0], |cpu, _| {
            cpu.regs.f = Z80_FLAG_ZERO
        }));
    }
    coverage::require_phases(&visited, "load", 0..10);
    coverage::require_phases(&visited, "stack", 0..14);
}

#[test]
fn single_prefix_cb_rmw_and_indexed_phase_families_continue_exactly() {
    let mut visited = BTreeSet::new();
    for program in [
        &[0x02][..],
        &[0xdd, 0x26, 0x35],
        &[0xdd, 0xfd, 0xdd, 0x26, 0x35],
        &[0xdd, 0xfd, 0xdd, 0xfd, 0xdd, 0xd3, 0x10],
        &[0xdd, 0xfd, 0xed, 0x41],
        &[0xcb, 0x06],
        &[0xcb, 0x46],
        &[0x34],
        &[0x35],
        &[0xed, 0x67],
        &[0xed, 0x6f],
        &[0xdd, 0x46, 0xfe],
        &[0xfd, 0x70, 2],
        &[0xdd, 0x86, 1],
        &[0xdd, 0x34, 0xff],
        &[0xfd, 0x35, 1],
        &[0xdd, 0x36, 2, 0x75],
        &[0xdd, 0xcb, 2, 0x00],
        &[0xfd, 0xcb, 0xff, 0x46],
        &[0xfd, 0xcb, 0xff, 0xc7],
    ] {
        visited.extend(check_instruction(program, |_, _| {}));
    }
    coverage::require_phases(&visited, "rmw", 0..3);
    coverage::require_phases(&visited, "indexed", 0..5);
    coverage::require_phases(&visited, "indexed_cb", 0..3);
    for key in ["prefix", "single", "cb_fetch"] {
        assert!(visited.contains(key));
    }
}

#[test]
fn ed_internal_io_and_block_phase_families_continue_exactly() {
    let mut visited = BTreeSet::new();
    for opcode in [
        0x42, 0x4a, 0x47, 0x4f, 0x57, 0x5f, 0x40, 0x41, 0x70, 0x71, 0x43, 0x4b, 0xa0, 0xa1, 0xa2,
        0xa3, 0xa8, 0xa9, 0xaa, 0xab, 0xb0, 0xb1, 0xb2, 0xb3, 0xb8, 0xb9, 0xba, 0xbb,
    ] {
        visited.extend(check_instruction(&[0xed, opcode, 0, 0x60], |_, _| {}));
    }
    coverage::require_phases(&visited, "internal", 0..2);
    coverage::require_phases(&visited, "block", 0..5);
    for key in ["ed_fetch", "io"] {
        assert!(visited.contains(key));
    }
}

#[test]
fn interrupt_modes_and_interrupt_operand_phases_continue_exactly() {
    let mut visited = check_instruction(&[0], |_, bus| bus.nmi = true);
    for (mode, opcode) in [
        (InterruptMode::Im0, 0),
        (InterruptMode::Im0, 0xff),
        (InterruptMode::Im0, 0x01),
        (InterruptMode::Im0, 0x06),
        (InterruptMode::Im0, 0xc3),
        (InterruptMode::Im0, 0xca),
        (InterruptMode::Im0, 0xcd),
        (InterruptMode::Im0, 0xcc),
        (InterruptMode::Im1, 0xff),
        (InterruptMode::Im2, 0xfe),
    ] {
        visited.extend(check_instruction(&[0], |cpu, bus| {
            cpu.interrupt_mode = mode;
            cpu.interrupt_flip_flop_1 = true;
            cpu.interrupt_flip_flop_2 = true;
            cpu.regs.i = 0x60;
            bus.irq = true;
            bus.acknowledge = opcode;
        }));
    }
    coverage::require_phases(&visited, "interrupt", 1..5);
    assert!(visited.contains("fetch.1"));
}

#[test]
fn interrupt_acknowledgement_phase_roundtrip_preserves_the_bus_effects() {
    for mode in [InterruptMode::Im0, InterruptMode::Im1, InterruptMode::Im2] {
        for nmi in [false, true] {
            let mut cpu = Cpu::new();
            cpu.interrupt_mode = mode;
            cpu.interrupt_flip_flop_1 = true;
            let mut memory = Bus::new(&[0]);
            memory.irq = true;
            memory.nmi = nmi;
            let service = interrupt::Service::poll(&cpu, &memory).unwrap();
            let mut w = Writer::new();
            service.write_continuation(&mut w);
            let bytes = w.into_bytes();
            assert_eq!(bytes.last(), Some(&0));
            let restored = interrupt::Service::read_continuation(&mut Reader::new(&bytes)).unwrap();
            assert_eq!(service, restored);
            let mut restored_cpu = cpu.clone();
            let mut restored_memory = memory.clone();
            assert_eq!(
                service.execute(&mut cpu, &mut memory),
                restored.execute(&mut restored_cpu, &mut restored_memory)
            );
            assert_eq!(cpu, restored_cpu);
            assert_eq!(memory, restored_memory);
        }
    }
}

#[test]
fn reset_inspection_and_reachable_stale_fields_survive() {
    let mut cpu = Cpu::new_with_reset(ResetState::new(0x1234, 0xabcd));
    cpu.regs = Registers {
        a: 1,
        f: 2,
        b: 3,
        c: 4,
        d: 5,
        e: 6,
        h: 7,
        l: 8,
        ix: 0x1234,
        iy: 0x5678,
        sp: 0x9abc,
        pc: 0xdef0,
        i: 9,
        r: 10,
    };
    cpu.shadow = ShadowRegisters {
        a: 11,
        f: 12,
        b: 13,
        c: 14,
        d: 15,
        e: 16,
        h: 17,
        l: 18,
    };
    cpu.state = CpuState::Suspended;
    cpu.interrupt_mode = InterruptMode::Im2;
    cpu.interrupt_flip_flop_1 = true;
    cpu.interrupt_flip_flop_2 = true;
    cpu.cycles = u64::MAX;
    cpu.last_opcode_pc = 0xfedc;
    cpu.last_opcode = 0xed;
    cpu.instruction_bytes = [1, 2, 3, 4];
    cpu.instruction_byte_count = 2;
    cpu.last_m1_fetch_count = u8::MAX;
    cpu.last_step_was_interrupt = true;
    cpu.enable_interrupts_delay = 250;
    cpu.trap = Some(CpuTrap::UnsupportedPrefixedOpcode {
        pc: 17,
        prefix: 0xdd,
        opcode: 0xed,
    });
    let (mut restored, _) = restore(&encode(&cpu, &BusState::default()));
    assert_eq!(restored, cpu);
    cpu.reset();
    restored.reset();
    assert_eq!(restored, cpu);
    check_instruction(&[0], |cpu, bus| {
        cpu.interrupt_flip_flop_1 = true;
        bus.irq = true;
        bus.acknowledge = 0xed;
    });
    check_instruction(&[0x76], |_, _| {});
}

#[test]
fn legacy_default_bytes_stay_exact() {
    let cpu = Cpu::new_with_reset(ResetState::new(0x1234, 0xabcd));
    let mut w = Writer::new();
    cpu.write_state(&mut w);
    let mut expected = vec![0; 43];
    expected[12..16].copy_from_slice(&[0xcd, 0xab, 0x34, 0x12]);
    expected[39..41].copy_from_slice(&[0x34, 0x12]);
    assert_eq!(w.into_bytes(), expected);
}

#[test]
fn truncated_and_malformed_pairs_leave_both_targets_unchanged() {
    let mut source = Cpu::new();
    let mut source_bus = BusState::default();
    let mut memory = Bus::new(&[0x21, 0x34, 0x12]);
    source
        .step_bus_segment(&mut memory, &mut source_bus)
        .unwrap();
    let bytes = encode(&source, &source_bus);
    let mut target = Cpu::new_with_reset(ResetState::new(17, 35));
    let mut target_bus = BusState::default();
    let mut target_memory = Bus::new(&[0xdd, 0x34, 1]);
    target.regs.pc = 0;
    target
        .step_bus_segment(&mut target_memory, &mut target_bus)
        .unwrap();
    let before = (target.clone(), target_bus);
    let mut reject = |bad: &[u8]| {
        assert!(
            target
                .read_continuation(&mut target_bus, &mut Reader::new(bad))
                .is_err()
        );
        assert_eq!((target.clone(), target_bus), before);
    };
    for length in 0..bytes.len() {
        reject(&bytes[..length]);
    }
    for (offset, value) in [
        (0, 2),
        (2, 255),
        (36, 3),
        (37, 3),
        (38, 2),
        (39, 2),
        (52, 3),
        (53, 2),
        (58, 5),
        (60, 2),
        (61, 2),
        (64, 12),
        (65, 2),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        reject(&bad);
    }
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() = 4;
    reject(&bad);
    let mut trailing = bytes.clone();
    trailing.push(0);
    let length = u32::from_le_bytes(trailing[2..6].try_into().unwrap()) + 1;
    trailing[2..6].copy_from_slice(&length.to_le_bytes());
    reject(&trailing);
    let mut oversized = bytes;
    oversized[2..6].copy_from_slice(&257u32.to_le_bytes());
    reject(&oversized);
}

fn rejects<T>(bytes: &[u8], read: impl FnOnce(&mut Reader<'_>) -> anyhow::Result<T>) {
    assert!(
        read(&mut Reader::new(bytes)).is_err(),
        "accepted malformed bytes: {bytes:02x?}"
    );
}

#[test]
fn operation_tags_indexes_conditions_and_executable_opcodes_validate() {
    for bytes in [
        &[0, 255][..],
        &[0, 0, 8],
        &[0, 0, 6],
        &[0, 1, 0, 4],
        &[0, 1, 1, 2],
        &[0, 1, 2],
        &[0, 3, 4],
    ] {
        rejects(bytes, load::Load::read_continuation);
    }
    for bytes in [
        &[0, 4][..],
        &[1, 2],
        &[4],
        &[0, 0, 0, 0, 0, 0, 14],
        &[0, 0, 0, 0, 0, 0, 0, 1, 8],
        &[0, 0, 0, 0, 0, 0, 0, 2],
        &[0, 0, 0, 0, 0, 0, 2, 2],
    ] {
        rejects(bytes, stack::Stack::read_continuation);
    }
    for bytes in [
        &[0, 0, 1, 8][..],
        &[0, 0, 0, 3],
        &[0, 0, 0, 0, 4],
        &[0, 0, 0, 0, 3, 0x66, 0, 0],
        &[0, 0, 0, 0, 3, 0x67, 0, 2],
    ] {
        rejects(bytes, rmw::Rmw::read_continuation);
    }
    for bytes in [
        &[5][..],
        &[0, 5],
        &[0, 0, 8],
        &[0, 0, 6],
        &[0, 2, 8],
        &[0, 3, 0x36],
        &[1, 4, 0, 0],
        &[3, 4, 0, 0],
        &[3, 3, 0x34, 0, 0],
    ] {
        rejects(bytes, indexed_memory::IndexedMemory::read_continuation);
    }
    rejects(&[3], indexed_cb::IndexedCb::read_continuation);
    rejects(&[0, 8, 0], ed::Io::read_continuation);
    for bytes in [&[0, 1][..], &[0x42, 2], &[0x47, 0]] {
        rejects(bytes, ed::Internal::read_continuation);
    }
    for (opcode, phase) in [
        (0, 1),
        (0xa0, 0),
        (0xa1, 2),
        (0xa2, 3),
        (0xa0, 4),
        (0xb0, 5),
    ] {
        rejects(
            &[opcode, 0, 0, 0, 0, 0, 0, 0, phase, 0],
            ed::Block::read_continuation,
        );
    }
    for bytes in [&[0, 0, 2][..], &[0, 0, 1, 3, 0, 0], &[0, 0, 0, 5]] {
        rejects(bytes, interrupt::Service::read_continuation);
    }
    for bytes in [
        &[1, 0, 0, 0xcc, 0][..],
        &[0, 1, 0, 0, 12],
        &[0, 1, 0, 0, 0, 0xff, 0],
        &[0, 1, 0, 0, 0, 0x02, 1, 0xcc],
    ] {
        rejects(bytes, BusState::read_continuation);
    }
}
