use super::{
    Cpu, CpuBus, CpuStep, RESET_VECTOR_HIGH, RESET_VECTOR_LOW, Registers, SpeedMode, StatusFlags,
    physical_address_for_page,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BusAccess {
    Read(u32),
    DummyRead(u32),
    Write(u32, u8),
    DummyWrite(u32, u8),
    Idle,
}

#[derive(Default)]
struct TestBus {
    values: std::collections::HashMap<u32, u8>,
    accesses: Vec<BusAccess>,
}

impl CpuBus for TestBus {
    fn read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(BusAccess::Read(physical_addr));
        self.values.get(&physical_addr).copied().unwrap_or(0xFF)
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        self.accesses.push(BusAccess::Write(physical_addr, value));
        self.values.insert(physical_addr, value);
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(BusAccess::DummyRead(physical_addr));
        self.values.get(&physical_addr).copied().unwrap_or(0xFF)
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        self.accesses
            .push(BusAccess::DummyWrite(physical_addr, value));
        self.values.insert(physical_addr, value);
    }

    fn idle(&mut self) {
        self.accesses.push(BusAccess::Idle);
    }
}

#[test]
fn logical_addresses_use_the_selected_8k_mapping_register() {
    let mut cpu = Cpu::new();
    for (page, value) in [0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0xFF]
        .into_iter()
        .enumerate()
    {
        cpu.set_mapping_register(page, value);
    }

    assert_eq!(cpu.logical_to_physical(0x0000), 0x020000);
    assert_eq!(cpu.logical_to_physical(0x1FFF), 0x021FFF);
    assert_eq!(cpu.logical_to_physical(0x2000), 0x042000);
    assert_eq!(cpu.logical_to_physical(0xA123), 0x0CA123);
    assert_eq!(cpu.logical_to_physical(0xFFFF), 0x1FFFFF);
}

#[test]
fn cached_mapping_page_bases_match_the_original_formula_exhaustively() {
    let mut cpu = Cpu::new();

    for page in 0..8 {
        for mapping in 0..=u8::MAX {
            cpu.set_mapping_register(page, mapping);
            for offset in 0..=0x1FFF {
                let logical_addr = ((page as u16) << 13) | offset;
                assert_eq!(
                    cpu.logical_to_physical(logical_addr),
                    physical_address_for_page(logical_addr, mapping),
                    "page={page} mapping={mapping:02X} offset={offset:04X}"
                );
            }
        }
    }
}

#[test]
fn logical_reads_and_writes_expose_physical_bus_addresses() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.set_mapping_register(3, 0xA5);
    bus.values.insert(0x14_A123, 0x5A);

    assert_eq!(cpu.read(&mut bus, 0x6123), 0x5A);
    cpu.write(&mut bus, 0x6124, 0xC3);

    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0x14_A123),
            BusAccess::Write(0x14_A124, 0xC3)
        ]
    );
}

#[test]
fn reset_reads_the_fixed_physical_vector_and_preserves_other_mprs() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    for page in 0..8 {
        cpu.set_mapping_register(page, 0x80 + page as u8);
    }
    bus.values.insert(RESET_VECTOR_LOW, 0x34);
    bus.values.insert(RESET_VECTOR_HIGH, 0x12);

    cpu.reset(&mut bus);

    assert_eq!(cpu.registers().pc, 0x1234);
    assert_eq!(
        cpu.mapping_registers(),
        [0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0]
    );
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(RESET_VECTOR_LOW),
            BusAccess::Read(RESET_VECTOR_HIGH)
        ]
    );
}

#[test]
fn reset_updates_only_the_documented_status_and_speed_state() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().status = StatusFlags::CARRY
        | StatusFlags::ZERO
        | StatusFlags::DECIMAL
        | StatusFlags::MEMORY_OPERATION
        | StatusFlags::OVERFLOW
        | StatusFlags::NEGATIVE;
    cpu.set_speed_mode(SpeedMode::High);

    cpu.reset(&mut bus);

    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY
            | StatusFlags::ZERO
            | StatusFlags::INTERRUPT
            | StatusFlags::OVERFLOW
            | StatusFlags::NEGATIVE
    );
    assert_eq!(cpu.speed_mode(), SpeedMode::Low);
}

#[test]
fn nop_matches_the_published_single_step_bus_sequence() {
    let mut cpu = cpu_with_state(
        Registers {
            a: 175,
            x: 4,
            y: 180,
            sp: 194,
            pc: 21_367,
            status: StatusFlags::from_bits_retain(131),
        },
        [143, 153, 59, 242, 222, 18, 11, 141],
    );
    let mut bus = TestBus::default();
    bus.values.insert(488_311, 0xEA);
    bus.values.insert(488_312, 200);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(
        step,
        CpuStep {
            pc: 21_367,
            physical_pc: 488_311,
            opcode: 0xEA,
            cycles: 2,
        }
    );
    assert_eq!(cpu.registers().pc, 21_368);
    assert_eq!(
        bus.accesses,
        [BusAccess::Read(488_311), BusAccess::DummyRead(488_312)]
    );
}

#[test]
fn speed_control_instructions_match_published_cycle_sequences() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().pc = 0x2000;
    cpu.registers_mut().status = StatusFlags::MEMORY_OPERATION;
    cpu.set_mapping_register(1, 1);
    bus.values.insert(0x2000, 0xD4);
    bus.values.insert(0x2001, 0x54);
    bus.values.insert(0x2002, 0xEA);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 3);
    assert_eq!(cpu.speed_mode(), SpeedMode::High);
    assert!(
        !cpu.registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 3);
    assert_eq!(cpu.speed_mode(), SpeedMode::Low);
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0x2000),
            BusAccess::DummyRead(0x2001),
            BusAccess::Idle,
            BusAccess::Read(0x2001),
            BusAccess::DummyRead(0x2002),
            BusAccess::Idle,
        ]
    );
}

#[test]
fn set_preserves_the_memory_operation_flag_for_exactly_the_next_instruction() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    bus.values.insert(0, 0xF4);
    bus.values.insert(1, 0xEA);
    bus.values.insert(2, 0xEA);

    cpu.step(&mut bus).unwrap();
    assert!(
        cpu.registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );

    cpu.step(&mut bus).unwrap();
    assert!(
        !cpu.registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
}

#[test]
fn tma_ors_every_selected_mapping_register() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().pc = 0x1000;
    cpu.set_mapping_register(1, 0xAC);
    cpu.set_mapping_register(3, 0x33);
    bus.values.insert(0x1000, 0x43);
    bus.values.insert(0x1001, 0b0000_1010);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 4);
    assert_eq!(cpu.registers().a, 0xBF);
    assert_eq!(cpu.registers().pc, 0x1002);
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0x1000),
            BusAccess::Read(0x1001),
            BusAccess::Idle,
            BusAccess::Idle,
        ]
    );
}

#[test]
fn tam_writes_the_accumulator_to_every_selected_mapping_register() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().a = 0xA4;
    cpu.registers_mut().pc = 0x1000;
    bus.values.insert(0x1000, 0x53);
    bus.values.insert(0x1001, 0b0101_1101);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 5);
    assert_eq!(
        cpu.mapping_registers(),
        [0xA4, 0, 0xA4, 0xA4, 0xA4, 0, 0xA4, 0]
    );
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0x1000),
            BusAccess::Read(0x1001),
            BusAccess::Idle,
            BusAccess::Idle,
            BusAccess::Idle,
        ]
    );
}

#[test]
fn indirect_indexed_wraps_the_direct_page_pointer_through_mpr1() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().y = 1;
    cpu.set_mapping_register(1, 0xF8);
    cpu.set_mapping_register(2, 0x12);
    bus.values.insert(0, 0xB1);
    bus.values.insert(1, 0xFF);
    bus.values.insert(0x1F_00FF, 0xFF);
    bus.values.insert(0x1F_0000, 0x3F);
    bus.values.insert(0x02_4000, 0x80);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 7);
    assert_eq!(cpu.registers().a, 0x80);
    assert!(cpu.registers().status.contains(StatusFlags::NEGATIVE));
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0),
            BusAccess::Read(1),
            BusAccess::Idle,
            BusAccess::Read(0x1F_00FF),
            BusAccess::Read(0x1F_0000),
            BusAccess::Idle,
            BusAccess::Read(0x02_4000),
        ]
    );
}

#[test]
fn absolute_indexed_crossing_keeps_fixed_five_cycle_timing() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().x = 1;
    cpu.set_mapping_register(2, 0x34);
    bus.values.insert(0, 0xBD);
    bus.values.insert(1, 0xFF);
    bus.values.insert(2, 0x3F);
    bus.values.insert(0x06_8000, 0x5A);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 5);
    assert_eq!(cpu.registers().a, 0x5A);
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0),
            BusAccess::Read(1),
            BusAccess::Read(2),
            BusAccess::Idle,
            BusAccess::Read(0x06_8000),
        ]
    );
}

#[test]
fn binary_adc_updates_carry_overflow_negative_and_zero() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().a = 0x50;
    bus.values.insert(0, 0x69);
    bus.values.insert(1, 0x50);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 2);
    assert_eq!(cpu.registers().a, 0xA0);
    assert!(!cpu.registers().status.contains(StatusFlags::CARRY));
    assert!(cpu.registers().status.contains(StatusFlags::OVERFLOW));
    assert!(cpu.registers().status.contains(StatusFlags::NEGATIVE));
    assert!(!cpu.registers().status.contains(StatusFlags::ZERO));
    assert_eq!(bus.accesses, [BusAccess::Read(0), BusAccess::Read(1)]);
}

#[test]
fn decimal_adc_preserves_overflow_and_uses_the_extra_bus_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().a = 0x49;
    cpu.registers_mut().status = StatusFlags::DECIMAL | StatusFlags::OVERFLOW;
    bus.values.insert(0, 0x69);
    bus.values.insert(1, 0x51);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 3);
    assert_eq!(cpu.registers().a, 0);
    assert!(cpu.registers().status.contains(StatusFlags::CARRY));
    assert!(cpu.registers().status.contains(StatusFlags::ZERO));
    assert!(cpu.registers().status.contains(StatusFlags::OVERFLOW));
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0),
            BusAccess::Read(1),
            BusAccess::DummyRead(2),
        ]
    );
}

#[test]
fn set_redirects_the_next_adc_to_direct_page_x_and_preserves_a() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().a = 0x55;
    cpu.registers_mut().x = 0x10;
    bus.values.insert(0, 0xF4);
    bus.values.insert(1, 0x69);
    bus.values.insert(2, 0x02);
    bus.values.insert(0x10, 0x01);

    cpu.step(&mut bus).unwrap();
    bus.accesses.clear();
    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 5);
    assert_eq!(cpu.registers().a, 0x55);
    assert_eq!(bus.values.get(&0x10), Some(&0x03));
    assert!(
        !cpu.registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(1),
            BusAccess::Read(2),
            BusAccess::Read(0x10),
            BusAccess::Idle,
            BusAccess::Write(0x10, 0x03),
        ]
    );
}

#[test]
fn binary_and_decimal_sbc_update_their_distinct_flags() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().a = 0x80;
    cpu.registers_mut().status = StatusFlags::CARRY;
    bus.values.insert(0, 0xE9);
    bus.values.insert(1, 0x01);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(cpu.registers().a, 0x7F);
    assert!(cpu.registers().status.contains(StatusFlags::CARRY));
    assert!(cpu.registers().status.contains(StatusFlags::OVERFLOW));

    cpu.registers_mut().pc = 2;
    cpu.registers_mut().a = 0;
    cpu.registers_mut().status = StatusFlags::CARRY | StatusFlags::DECIMAL | StatusFlags::OVERFLOW;
    bus.values.insert(2, 0xE9);
    bus.values.insert(3, 0x01);
    bus.accesses.clear();

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 3);
    assert_eq!(cpu.registers().a, 0x99);
    assert!(!cpu.registers().status.contains(StatusFlags::CARRY));
    assert!(cpu.registers().status.contains(StatusFlags::NEGATIVE));
    assert!(cpu.registers().status.contains(StatusFlags::OVERFLOW));
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(2),
            BusAccess::Read(3),
            BusAccess::DummyRead(4),
        ]
    );
}

#[test]
fn cpx_updates_compare_flags_without_changing_x() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.registers_mut().x = 0;
    bus.values.insert(0, 0xE0);
    bus.values.insert(1, 1);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(cpu.registers().x, 0);
    assert!(!cpu.registers().status.contains(StatusFlags::CARRY));
    assert!(!cpu.registers().status.contains(StatusFlags::ZERO));
    assert!(cpu.registers().status.contains(StatusFlags::NEGATIVE));
}

#[test]
fn direct_page_asl_uses_mpr1_and_the_huc6280_rmw_sequence() {
    let mut cpu = Cpu::new();
    let mut bus = TestBus::default();
    cpu.set_mapping_register(1, 0xAB);
    bus.values.insert(0, 0x06);
    bus.values.insert(1, 0x34);
    bus.values.insert(0x15_6034, 0x81);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 6);
    assert_eq!(bus.values.get(&0x15_6034), Some(&0x02));
    assert!(cpu.registers().status.contains(StatusFlags::CARRY));
    assert_eq!(
        bus.accesses,
        [
            BusAccess::Read(0),
            BusAccess::Read(1),
            BusAccess::Idle,
            BusAccess::Read(0x15_6034),
            BusAccess::Idle,
            BusAccess::Write(0x15_6034, 0x02),
        ]
    );
}

#[test]
fn set_redirects_logical_operations_to_direct_page_x() {
    for (opcode, value, operand, expected) in [
        (0x09, 0x50, 0x0F, 0x5F),
        (0x29, 0xF3, 0x0F, 0x03),
        (0x49, 0xF0, 0x0F, 0xFF),
    ] {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::default();
        cpu.registers_mut().a = 0xA5;
        cpu.registers_mut().x = 0x10;
        bus.values.insert(0, 0xF4);
        bus.values.insert(1, opcode);
        bus.values.insert(2, operand);
        bus.values.insert(0x10, value);

        cpu.step(&mut bus).unwrap();
        let step = cpu.step(&mut bus).unwrap();

        assert_eq!(step.cycles, 5);
        assert_eq!(cpu.registers().a, 0xA5);
        assert_eq!(bus.values.get(&0x10), Some(&expected));
    }
}

fn cpu_with_state(registers: Registers, mpr: [u8; 8]) -> Cpu {
    let mut cpu = Cpu::new();
    *cpu.registers_mut() = registers;
    for (index, value) in mpr.into_iter().enumerate() {
        cpu.set_mapping_register(index, value);
    }
    cpu
}
