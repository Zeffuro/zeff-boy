use super::super::{BaseBus, BaseBusDevices};
use super::{
    Cpu, CpuBus, HuC6280, InterruptSource, IrqPort, LineLevel, OnChipIo, PHYSICAL_ADDRESS_MASK,
    PROVISIONAL_INTERRUPT_ENTRY_CYCLES, SpeedMode, StatusFlags, TIMER_MASTER_TICKS, TimerPort,
    UNINITIALIZED_TIMER_COUNTER_READ,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservedAccess {
    Read(u32, u8, bool),
    Write(u32, u8, bool),
}

#[derive(Default)]
struct Devices {
    observed: Vec<ObservedAccess>,
    external_timer_accesses: u32,
    external_irq_accesses: u32,
}

impl BaseBusDevices for Devices {
    fn read_timer_counter(&mut self) -> u8 {
        self.external_timer_accesses += 1;
        0xEE
    }

    fn write_timer(&mut self, _port: TimerPort, _value: u8) {
        self.external_timer_accesses += 1;
    }

    fn read_irq(&mut self, _port: IrqPort) -> u8 {
        self.external_irq_accesses += 1;
        0xEE
    }

    fn write_irq(&mut self, _port: IrqPort, _value: u8) {
        self.external_irq_accesses += 1;
    }

    fn observe_internal_read(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.observed
            .push(ObservedAccess::Read(physical_addr, value, dummy));
    }

    fn observe_internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.observed
            .push(ObservedAccess::Write(physical_addr, value, dummy));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceAccess {
    Read(u32, u8),
    DummyRead(u32, u8),
    Write(u32, u8),
    Idle,
}

struct TraceBus {
    memory: Box<[u8]>,
    trace: Vec<TraceAccess>,
}

impl TraceBus {
    fn new() -> Self {
        Self {
            memory: vec![0; PHYSICAL_ADDRESS_MASK as usize + 1].into_boxed_slice(),
            trace: Vec::new(),
        }
    }

    fn set(&mut self, physical_addr: u32, value: u8) {
        self.memory[physical_addr as usize] = value;
    }
}

impl CpuBus for TraceBus {
    fn read(&mut self, physical_addr: u32) -> u8 {
        let value = self.memory[physical_addr as usize];
        self.trace.push(TraceAccess::Read(physical_addr, value));
        value
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        self.memory[physical_addr as usize] = value;
        self.trace.push(TraceAccess::Write(physical_addr, value));
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        let value = self.memory[physical_addr as usize];
        self.trace
            .push(TraceAccess::DummyRead(physical_addr, value));
        value
    }

    fn idle(&mut self) {
        self.trace.push(TraceAccess::Idle);
    }
}

#[test]
fn timer_uses_a_3072_master_tick_cadence_and_periodically_reloads() {
    let mut io = OnChipIo::new();
    assert_eq!(io.read_timer_counter(), UNINITIALIZED_TIMER_COUNTER_READ);

    io.write_timer(TimerPort::CounterReload, 0x82);
    assert_eq!(io.timer_reload(), 2);
    io.write_timer(TimerPort::Control, 1);
    assert_eq!(io.read_timer_counter(), 2);

    io.advance_master_ticks(TIMER_MASTER_TICKS - 1);
    assert_eq!(io.read_timer_counter(), 2);
    io.advance_master_ticks(1);
    assert_eq!(io.read_timer_counter(), 1);
    io.advance_master_ticks(TIMER_MASTER_TICKS);
    assert_eq!(io.read_timer_counter(), 0);
    assert!(!io.timer_irq_pending());
    io.advance_master_ticks(TIMER_MASTER_TICKS);
    assert_eq!(io.read_timer_counter(), 2);
    assert!(io.timer_irq_pending());

    io.write_irq(IrqPort::Request, 0xFF);
    assert!(!io.timer_irq_pending());
    io.advance_master_ticks(7 * TIMER_MASTER_TICKS);
    assert_eq!(io.read_timer_counter(), 1);
    assert!(io.timer_irq_pending());

    let mut zero_period = OnChipIo::new();
    zero_period.write_timer(TimerPort::CounterReload, 0);
    zero_period.write_timer(TimerPort::Control, 1);
    zero_period.advance_master_ticks(TIMER_MASTER_TICKS - 1);
    assert!(!zero_period.timer_irq_pending());
    zero_period.advance_master_ticks(1);
    assert!(zero_period.timer_irq_pending());
    assert_eq!(zero_period.read_timer_counter(), 0);
}

#[test]
fn timer_start_stop_restart_and_reload_writes_preserve_the_documented_phase() {
    let mut io = OnChipIo::new();
    io.write_timer(TimerPort::CounterReload, 5);
    io.write_timer(TimerPort::Control, 1);
    io.advance_master_ticks(TIMER_MASTER_TICKS + 100);
    assert_eq!(io.read_timer_counter(), 4);
    assert_eq!(io.timer_prescaler_ticks(), 100);

    io.write_timer(TimerPort::Control, 0xFF);
    assert_eq!(io.read_timer_counter(), 4);
    assert_eq!(io.timer_prescaler_ticks(), 100);

    io.write_timer(TimerPort::CounterReload, 2);
    io.advance_master_ticks(4 * TIMER_MASTER_TICKS);
    assert_eq!(io.read_timer_counter(), 0);
    io.advance_master_ticks(TIMER_MASTER_TICKS - 100);
    assert_eq!(io.read_timer_counter(), 2);
    assert!(io.timer_irq_pending());

    io.advance_master_ticks(1_000);
    io.write_timer(TimerPort::Control, 0xFE);
    assert!(!io.timer_running());
    assert_eq!(io.timer_prescaler_ticks(), 0);
    io.advance_master_ticks(20 * TIMER_MASTER_TICKS);
    assert_eq!(io.read_timer_counter(), 2);

    io.write_timer(TimerPort::Control, 1);
    io.advance_master_ticks(TIMER_MASTER_TICKS - 1);
    assert_eq!(io.read_timer_counter(), 2);
    io.advance_master_ticks(1);
    assert_eq!(io.read_timer_counter(), 1);
}

#[test]
fn interrupt_masks_acknowledgement_levels_and_priority_are_independent() {
    let mut io = OnChipIo::new();
    io.write_timer(TimerPort::CounterReload, 0);
    io.write_timer(TimerPort::Control, 1);
    io.write_irq(IrqPort::Disable, 0xFF);
    io.set_irq1_line(LineLevel::Low);
    io.set_irq2_line(LineLevel::Low);
    io.advance_master_ticks(TIMER_MASTER_TICKS);

    assert_eq!(io.read_irq(IrqPort::Disable), 0x07);
    assert_eq!(io.read_irq(IrqPort::Request), 0x07);
    assert_eq!(io.highest_priority_unmasked_request(), None);
    assert!(io.timer_irq_pending());

    io.write_irq(IrqPort::Disable, 0);
    assert_eq!(
        io.highest_priority_unmasked_request(),
        Some(InterruptSource::Timer)
    );
    io.write_irq(IrqPort::Request, 0xA5);
    assert_eq!(io.read_irq(IrqPort::Request), 0x03);
    assert_eq!(
        io.highest_priority_unmasked_request(),
        Some(InterruptSource::Irq1)
    );
    io.set_irq1_line(LineLevel::High);
    assert_eq!(
        io.highest_priority_unmasked_request(),
        Some(InterruptSource::Irq2)
    );
    assert_eq!(io.read_irq(IrqPort::Request), 0x01);
    io.write_irq(IrqPort::Request, 0);
    assert_eq!(io.read_irq(IrqPort::Request), 0x01);
}

#[test]
fn nmi_latches_only_high_to_low_edges_and_queries_are_non_destructive() {
    let mut io = OnChipIo::new();
    assert!(!io.nmi_pending());

    io.set_nmi_line(LineLevel::Low);
    assert_eq!(
        io.highest_priority_unmasked_request(),
        Some(InterruptSource::Nmi)
    );
    assert_eq!(
        io.highest_priority_unmasked_request(),
        Some(InterruptSource::Nmi)
    );
    assert!(io.nmi_pending());

    io.reset();
    assert!(!io.nmi_pending());
    io.set_nmi_line(LineLevel::Low);
    assert!(!io.nmi_pending());
    io.set_nmi_line(LineLevel::High);
    io.set_nmi_line(LineLevel::Low);
    assert!(io.nmi_pending());

    let mut priorities = OnChipIo::new();
    priorities.set_irq1_line(LineLevel::Low);
    priorities.write_timer(TimerPort::CounterReload, 0);
    priorities.write_timer(TimerPort::Control, 1);
    priorities.advance_master_ticks(TIMER_MASTER_TICKS);
    priorities.set_nmi_line(LineLevel::Low);
    assert_eq!(
        priorities.highest_priority_unmasked_request(),
        Some(InterruptSource::Nmi)
    );
}

#[test]
fn provisional_interrupt_entry_uses_logical_stack_and_vector_mappings() {
    let mut cpu = HuC6280::new();
    cpu.cpu_mut().set_mapping_register(1, 0xF8);
    cpu.cpu_mut().set_mapping_register(7, 0x20);
    cpu.cpu_mut().registers_mut().pc = 0x3456;
    cpu.cpu_mut().registers_mut().sp = 0;
    cpu.cpu_mut().registers_mut().status = StatusFlags::CARRY
        | StatusFlags::DECIMAL
        | StatusFlags::BREAK
        | StatusFlags::MEMORY_OPERATION;
    cpu.on_chip_io_mut()
        .write_timer(TimerPort::CounterReload, 0);
    cpu.on_chip_io_mut().write_timer(TimerPort::Control, 1);
    cpu.advance_master_ticks(TIMER_MASTER_TICKS);
    cpu.sample_interrupts_for_test(false);

    let mut bus = TraceBus::new();
    bus.set(0x41FFA, 0x78);
    bus.set(0x41FFB, 0x56);
    let step = cpu.service_interrupt_boundary(&mut bus).unwrap();

    assert_eq!(step.source, InterruptSource::Timer);
    assert_eq!(step.cycles, PROVISIONAL_INTERRUPT_ENTRY_CYCLES);
    assert_eq!(cpu.cpu().registers().pc, 0x5678);
    assert_eq!(cpu.cpu().registers().sp, 0xFD);
    assert_eq!(
        cpu.cpu().registers().status,
        StatusFlags::CARRY | StatusFlags::INTERRUPT
    );
    assert!(cpu.on_chip_io().timer_irq_pending());
    assert_eq!(
        bus.trace,
        [
            TraceAccess::DummyRead(0x1F_1456, 0),
            TraceAccess::DummyRead(0x1F_1457, 0),
            TraceAccess::Write(0x1F_0100, 0x34),
            TraceAccess::Write(0x1F_01FF, 0x56),
            TraceAccess::Write(0x1F_01FE, 0x29),
            TraceAccess::Read(0x04_1FFA, 0x78),
            TraceAccess::Read(0x04_1FFB, 0x56),
            TraceAccess::Idle,
        ]
    );
}

#[test]
fn boundary_service_uses_sampled_i_and_transfers_only_selected_nmi_edges() {
    let mut cpu = HuC6280::new();
    let mut bus = TraceBus::new();
    cpu.cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);
    cpu.set_irq1_line(LineLevel::Low);

    cpu.sample_interrupts_for_test(true);
    assert_eq!(cpu.service_interrupt_boundary(&mut bus), None);
    assert!(bus.trace.is_empty());
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Request), 0x02);

    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(true);
    assert!(!cpu.on_chip_io().nmi_pending());
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Nmi
    );
    assert!(!cpu.on_chip_io().nmi_pending());

    cpu.set_irq1_line(LineLevel::High);
    cpu.cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    bus.trace.clear();
    cpu.sample_interrupts_for_test(false);
    assert_eq!(cpu.service_interrupt_boundary(&mut bus), None);
    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(cpu.service_interrupt_boundary(&mut bus), None);
    cpu.set_nmi_line(LineLevel::High);
    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Nmi
    );
}

#[test]
fn boundary_service_preserves_maskable_requests_and_applies_priority() {
    let mut cpu = HuC6280::new();
    let mut bus = TraceBus::new();
    cpu.on_chip_io_mut()
        .write_timer(TimerPort::CounterReload, 0);
    cpu.on_chip_io_mut().write_timer(TimerPort::Control, 1);
    cpu.advance_master_ticks(TIMER_MASTER_TICKS);
    cpu.set_irq1_line(LineLevel::Low);
    cpu.set_irq2_line(LineLevel::Low);
    cpu.set_nmi_line(LineLevel::Low);

    assert_eq!(
        cpu.highest_priority_unmasked_request(),
        Some(InterruptSource::Nmi)
    );
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Nmi
    );
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Request), 0x07);

    cpu.cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Timer
    );
    assert!(cpu.on_chip_io().timer_irq_pending());

    cpu.cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    cpu.on_chip_io_mut().write_irq(IrqPort::Request, 0);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Irq1
    );
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Request), 0x03);

    cpu.cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    cpu.set_irq1_line(LineLevel::High);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Irq2
    );
    cpu.cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    cpu.sample_interrupts_for_test(false);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Irq2
    );
}

#[test]
fn instruction_interrupt_polls_use_the_contract_i_value() {
    let mut cli = HuC6280::new();
    let mut cli_bus = TraceBus::new();
    cli_bus.set(0, 0x58);
    cli_bus.set(1, 0xEA);
    cli.cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);
    cli.set_irq1_line(LineLevel::Low);
    cli.step_instruction(&mut cli_bus).unwrap();
    cli.sample_interrupts_after_action();
    assert_eq!(cli.sampled_interrupt(), None);
    cli.step_instruction(&mut cli_bus).unwrap();
    cli.sample_interrupts_after_action();
    assert_eq!(cli.sampled_interrupt(), Some(InterruptSource::Irq1));

    let mut sei = HuC6280::new();
    let mut sei_bus = TraceBus::new();
    sei_bus.set(0, 0x78);
    sei_bus.set(0x1FF8, 0x00);
    sei_bus.set(0x1FF9, 0x02);
    sei.set_irq1_line(LineLevel::Low);
    sei.step_instruction(&mut sei_bus).unwrap();
    sei.sample_interrupts_after_action();
    assert_eq!(sei.sampled_interrupt(), Some(InterruptSource::Irq1));
    assert_eq!(
        sei.service_interrupt_boundary(&mut sei_bus).unwrap().source,
        InterruptSource::Irq1
    );

    let mut plp = HuC6280::new();
    let mut plp_bus = TraceBus::new();
    plp_bus.set(0, 0x28);
    plp_bus.set(0x0100, 0);
    plp.cpu_mut().registers_mut().sp = 0xFF;
    plp.cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);
    plp.set_irq1_line(LineLevel::Low);
    plp.step_instruction(&mut plp_bus).unwrap();
    plp.sample_interrupts_after_action();
    assert_eq!(plp.sampled_interrupt(), None);
    assert!(
        !plp.cpu()
            .registers()
            .status
            .contains(StatusFlags::INTERRUPT)
    );

    let mut plp_masked = HuC6280::new();
    let mut plp_masked_bus = TraceBus::new();
    plp_masked_bus.set(0, 0x28);
    plp_masked_bus.set(0x0100, StatusFlags::INTERRUPT.bits());
    plp_masked.cpu_mut().registers_mut().sp = 0xFF;
    plp_masked.set_irq1_line(LineLevel::Low);
    plp_masked.step_instruction(&mut plp_masked_bus).unwrap();
    plp_masked.sample_interrupts_after_action();
    assert!(
        plp_masked
            .cpu()
            .registers()
            .status
            .contains(StatusFlags::INTERRUPT)
    );
    assert_eq!(plp_masked.sampled_interrupt(), Some(InterruptSource::Irq1));

    let mut rti = HuC6280::new();
    let mut rti_bus = TraceBus::new();
    rti_bus.set(0, 0x40);
    rti_bus.set(0x01FD, 0);
    rti_bus.set(0x01FE, 0x34);
    rti_bus.set(0x01FF, 0x12);
    rti.cpu_mut().registers_mut().sp = 0xFC;
    rti.cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);
    rti.set_irq1_line(LineLevel::Low);
    rti.step_instruction(&mut rti_bus).unwrap();
    rti.sample_interrupts_after_action();
    assert_eq!(rti.cpu().registers().pc, 0x1234);
    assert_eq!(rti.sampled_interrupt(), Some(InterruptSource::Irq1));
}

#[test]
fn nmi_edge_during_entry_is_sampled_after_the_first_entry() {
    let mut cpu = HuC6280::new();
    let mut bus = TraceBus::new();
    bus.set(0x1FFC, 0x00);
    bus.set(0x1FFD, 0x02);
    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(true);
    assert_eq!(
        cpu.service_interrupt_boundary(&mut bus).unwrap().source,
        InterruptSource::Nmi
    );

    cpu.set_nmi_line(LineLevel::High);
    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_after_action();
    assert_eq!(cpu.sampled_interrupt(), Some(InterruptSource::Nmi));
}

#[test]
fn provisional_interrupt_entry_dummy_reads_wrap_the_logical_pc() {
    let mut cpu = HuC6280::new();
    let mut bus = TraceBus::new();
    cpu.cpu_mut().set_mapping_register(0, 0x10);
    cpu.cpu_mut().set_mapping_register(1, 0x20);
    cpu.cpu_mut().set_mapping_register(7, 0x30);
    cpu.cpu_mut().registers_mut().pc = 0xFFFF;
    cpu.cpu_mut().registers_mut().sp = 0xFF;
    bus.set(0x061FF8, 0x34);
    bus.set(0x061FF9, 0x12);
    cpu.set_irq1_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(false);

    cpu.service_interrupt_boundary(&mut bus).unwrap();

    assert_eq!(bus.trace[0], TraceAccess::DummyRead(0x061FFF, 0));
    assert_eq!(bus.trace[1], TraceAccess::DummyRead(0x020000, 0));
    assert_eq!(cpu.cpu().registers().pc, 0x1234);
}

#[test]
fn interrupt_rti_round_trip_preserves_set_for_the_next_alu_instruction() {
    let mut cpu = HuC6280::new();
    let mut bus = TraceBus::new();
    cpu.cpu_mut().set_mapping_register(1, 0xF8);
    cpu.cpu_mut().registers_mut().a = 0x77;
    cpu.cpu_mut().registers_mut().x = 0x10;
    cpu.cpu_mut().registers_mut().sp = 0xFF;
    bus.set(0, 0xF4);
    bus.set(1, 0x69);
    bus.set(2, 3);
    bus.set(0x0100, 0x40);
    bus.set(0x1FFC, 0x00);
    bus.set(0x1FFD, 0x01);
    bus.set(0x1F_0010, 5);

    cpu.step_instruction(&mut bus).unwrap();
    cpu.sample_interrupts_after_action();
    assert!(
        cpu.cpu()
            .registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
    cpu.set_nmi_line(LineLevel::Low);
    cpu.sample_interrupts_for_test(false);
    cpu.service_interrupt_boundary(&mut bus).unwrap();
    cpu.sample_interrupts_after_action();
    assert!(
        !cpu.cpu()
            .registers()
            .status
            .intersects(StatusFlags::DECIMAL | StatusFlags::BREAK | StatusFlags::MEMORY_OPERATION)
    );

    cpu.step_instruction(&mut bus).unwrap();
    cpu.sample_interrupts_after_action();
    assert_eq!(cpu.cpu().registers().pc, 1);
    assert!(
        cpu.cpu()
            .registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
    assert_eq!(cpu.service_interrupt_boundary(&mut bus), None);

    let step = cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(step.cycles, 5);
    assert_eq!(cpu.cpu().registers().a, 0x77);
    assert_eq!(bus.memory[0x1F_0010], 8);
    assert!(
        !cpu.cpu()
            .registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
}

#[test]
fn explicit_master_ticks_are_independent_of_cpu_speed_mode() {
    let mut low = HuC6280::new();
    let mut high = HuC6280::new();
    high.cpu_mut().set_speed_mode(SpeedMode::High);
    assert_eq!(low.cpu().speed_mode(), SpeedMode::Low);

    for cpu in [&mut low, &mut high] {
        cpu.on_chip_io_mut()
            .write_timer(TimerPort::CounterReload, 3);
        cpu.on_chip_io_mut().write_timer(TimerPort::Control, 1);
        cpu.advance_master_ticks(5 * TIMER_MASTER_TICKS + 123);
    }

    assert_eq!(
        low.on_chip_io().read_timer_counter(),
        high.on_chip_io().read_timer_counter()
    );
    assert_eq!(
        low.on_chip_io().timer_prescaler_ticks(),
        high.on_chip_io().timer_prescaler_ticks()
    );
    assert_eq!(
        low.on_chip_io().timer_irq_pending(),
        high.on_chip_io().timer_irq_pending()
    );
}

#[test]
fn wrapper_intercepts_timer_and_irq_mirrors_before_the_external_base_bus() {
    let rom = vec![
        0xA9, 0x82, 0x8D, 0x00, 0x4E, 0xA9, 0xFF, 0x8D, 0x01, 0x4E, 0xAD, 0x00, 0x4C, 0xAD, 0x01,
        0x4C, 0xA9, 0xFF, 0x8D, 0x02, 0x54, 0xAD, 0x02, 0x54, 0xA9, 0x00, 0x8D, 0x03, 0x54,
    ];
    let mut bus = BaseBus::new(rom, Devices::default()).unwrap();
    let mut cpu = HuC6280::new();
    cpu.cpu_mut().set_mapping_register(2, 0xFF);

    for _ in 0..11 {
        cpu.step_instruction(&mut bus).unwrap();
        cpu.sample_interrupts_after_action();
    }

    assert_eq!(cpu.on_chip_io().timer_reload(), 2);
    assert!(cpu.on_chip_io().timer_running());
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Disable), 7);
    assert_eq!(bus.devices().external_timer_accesses, 0);
    assert_eq!(bus.devices().external_irq_accesses, 0);
    assert_eq!(
        bus.devices().observed,
        [
            ObservedAccess::Write(0x1F_EE00, 0x82, false),
            ObservedAccess::Write(0x1F_EE01, 0xFF, false),
            ObservedAccess::Read(0x1F_EC00, 2, false),
            ObservedAccess::Read(0x1F_EC01, 0xFF, false),
            ObservedAccess::Write(0x1F_F402, 0xFF, false),
            ObservedAccess::Read(0x1F_F402, 7, false),
            ObservedAccess::Write(0x1F_F403, 0, false),
        ]
    );
}

#[test]
fn wrapper_observes_internal_dummy_reads_once() {
    let mut bus = BaseBus::new(Vec::new(), Devices::default()).unwrap();
    let mut cpu = HuC6280::new();
    cpu.cpu_mut().set_mapping_register(0, 0xFF);
    cpu.cpu_mut().registers_mut().pc = 0x1402;
    cpu.on_chip_io_mut().write_irq(IrqPort::Disable, 2);

    cpu.step_instruction(&mut bus).unwrap();

    assert_eq!(
        bus.devices().observed,
        [
            ObservedAccess::Read(0x1F_F402, 2, false),
            ObservedAccess::Read(0x1F_F403, 0, true),
        ]
    );
    assert_eq!(bus.devices().external_irq_accesses, 0);
}

#[test]
fn raw_cpu_leaves_top_bank_device_accesses_to_its_bus() {
    let mut bus = BaseBus::new(vec![0xAD, 0x00, 0x4C], Devices::default()).unwrap();
    let mut cpu = Cpu::new();
    cpu.set_mapping_register(2, 0xFF);

    cpu.step(&mut bus).unwrap();

    assert_eq!(cpu.registers().a, 0xEE);
    assert_eq!(bus.devices().external_timer_accesses, 1);
    assert!(bus.devices().observed.is_empty());
}

#[test]
fn reset_clears_internal_state_but_preserves_external_line_levels() {
    let mut rom = vec![0xFF; 0x2000];
    rom[0x1FFE] = 0x34;
    rom[0x1FFF] = 0x12;
    let mut bus = BaseBus::new(rom, ()).unwrap();
    let mut cpu = HuC6280::new();
    cpu.set_irq1_line(LineLevel::Low);
    cpu.set_irq2_line(LineLevel::Low);
    cpu.set_nmi_line(LineLevel::Low);
    cpu.on_chip_io_mut()
        .write_timer(TimerPort::CounterReload, 5);
    cpu.on_chip_io_mut().write_timer(TimerPort::Control, 1);
    cpu.on_chip_io_mut().write_irq(IrqPort::Disable, 7);

    cpu.reset(&mut bus);

    assert_eq!(cpu.cpu().registers().pc, 0x1234);
    assert_eq!(
        cpu.on_chip_io().read_timer_counter(),
        UNINITIALIZED_TIMER_COUNTER_READ
    );
    assert!(!cpu.on_chip_io().timer_running());
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Disable), 0);
    assert_eq!(cpu.on_chip_io().read_irq(IrqPort::Request), 3);
    assert!(!cpu.on_chip_io().nmi_pending());
}
