use super::cpu::{
    CpuTrap, InterruptSource, IrqPort, LineLevel, SpeedMode, StatusFlags, TIMER_MASTER_TICKS,
    VdcPort,
};
use super::machine::checked_clock_add;
use super::{
    BASE_PCE_NO_CD_CONTROLLER_UPPER_BITS, BASE_TURBOGRAFX16_NO_CD_CONTROLLER_UPPER_BITS, BaseBus,
    ControllerPort, HUCARD_ROM_REGION_LEN, HuC6280Psg, MAX_PSG_SAMPLE_RATE,
    PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_RGBA_BYTES, PCE_ACTIVE_FRAME_UNUSED_RGBA,
    PCE_ACTIVE_FRAME_WIDTH, PCE_SIGNAL_FIRST_ROW, PCE_SIGNAL_ROW_END,
    PCE_VDC_VCE_ACCESS_WAIT_CYCLES, PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE,
    PROVISIONAL_PCE_VSYNC_ASSERT_NORMALIZED_TO_LINE_ZERO,
    PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS,
    PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC,
    PSG_CLOCK_DENOMINATOR, PSG_CLOCK_NUMERATOR, PSG_INTERNAL_MASTER_CLOCK_DIVISOR,
    PSG_MASTER_CLOCK_DIVISOR, PceCartridgeDescriptor, PceCartridgeHardware, PceClockCounter,
    PceConsoleWiring, PceCpuAction, PceDevices, PceExecutionState, PceMachine, PceMachineError,
    PsgPort, PsgRevision, VDC_SATB_WORDS, VceFrameLength, VcePixelClock, VcePort, VdcDmaChannel,
    VdcDmaProgress, VdcRegister, VdcStatus,
};
use zeff_emu_common::debug::{
    DebugEvent, TraceExecMode, TraceWriteKind, TraceWriteWidth, WatchType,
};

const RESET_PC: u16 = 0xE000;

fn rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..program.len()].copy_from_slice(program);
    set_vector(&mut rom, 0x1FFE, RESET_PC);
    rom
}

fn set_vector(rom: &mut [u8], offset: usize, address: u16) {
    let [low, high] = address.to_le_bytes();
    rom[offset] = low;
    rom[offset + 1] = high;
}

fn sha256(hex: &str) -> [u8; 32] {
    let mut hash = [0; 32];
    for (byte, digits) in hash.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap();
    }
    hash
}

fn write_vdc_register(devices: &mut PceDevices, register: VdcRegister, value: u16) {
    let vdc = devices.vdc_mut();
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn configure_external_262(devices: &mut PceDevices, control: u16) {
    write_vdc_register(devices, VdcRegister::Control, control);
    write_vdc_register(devices, VdcRegister::VerticalSync, 0);
    write_vdc_register(devices, VdcRegister::VerticalDisplay, 257);
    write_vdc_register(devices, VdcRegister::VerticalDisplayEnd, 1);
}

fn advance_until_satb_dma_finishes(machine: &mut PceMachine) {
    for _ in 0..100_000 {
        if machine.devices().vdc().pending_satb_dma().is_none()
            && machine.devices().vdc().active_satb_dma().is_none()
        {
            return;
        }
        machine.step_boundary().unwrap();
    }
    panic!("SATB DMA did not finish");
}

fn configure_external_1943_profile(devices: &mut PceDevices) {
    write_vdc_register(devices, VdcRegister::Control, 0x0008);
    write_vdc_register(devices, VdcRegister::VerticalSync, 0x0F02);
    write_vdc_register(devices, VdcRegister::VerticalDisplay, 0x00EF);
    write_vdc_register(devices, VdcRegister::VerticalDisplayEnd, 0x0004);
}

fn high_speed_loop_rom() -> Vec<u8> {
    rom_with_program(&[0xD4, 0xEA, 0x80, 0xFD])
}

fn nonblack_video_rom() -> Vec<u8> {
    rom_with_program(&[
        0x03, 0x0C, 0x13, 0x00, 0x23, 0x00, 0x03, 0x0D, 0x13, 0x01, 0x23, 0x01, 0x03, 0x0E, 0x13,
        0x01, 0x23, 0x00, 0xA9, 0xFF, 0x53, 0x01, 0xA9, 0x00, 0x8D, 0x02, 0x04, 0x8D, 0x03, 0x04,
        0xA9, 0x38, 0x8D, 0x04, 0x04, 0xA9, 0x00, 0x8D, 0x05, 0x04, 0xD4, 0xEA, 0x80, 0xFD,
    ])
}

fn set_red_backdrop(devices: &mut PceDevices) {
    let vce = devices.vce_mut();
    vce.write_port(VcePort::from_offset(2), 0);
    vce.write_port(VcePort::from_offset(3), 0);
    vce.write_port(VcePort::from_offset(4), 0x38);
    vce.write_port(VcePort::from_offset(5), 0);
}

fn psg_port(offset: u8) -> PsgPort {
    PsgPort::from_offset(offset)
}

fn configure_dda(psg: &mut HuC6280Psg, sample: u8) {
    psg.write_port(psg_port(0), 0);
    psg.write_port(psg_port(1), 0xFF);
    psg.write_port(psg_port(5), 0xFF);
    psg.write_port(psg_port(4), 0xDF);
    psg.write_port(psg_port(6), sample);
}

fn configure_square_psg(psg: &mut HuC6280Psg) {
    psg.write_port(psg_port(0), 0);
    psg.write_port(psg_port(1), 0xFF);
    psg.write_port(psg_port(2), 64);
    psg.write_port(psg_port(3), 0);
    psg.write_port(psg_port(5), 0xFF);
    for _ in 0..16 {
        psg.write_port(psg_port(6), 0);
    }
    for _ in 0..16 {
        psg.write_port(psg_port(6), 31);
    }
    psg.write_port(psg_port(4), 0x9F);
}

fn expected_audio_frames(master_ticks: u64, sample_rate: u32) -> usize {
    let psg_ticks = master_ticks / PSG_MASTER_CLOCK_DIVISOR;
    ((u128::from(psg_ticks) * u128::from(sample_rate) * u128::from(PSG_CLOCK_DENOMINATOR))
        / u128::from(PSG_CLOCK_NUMERATOR)) as usize
}

fn drain_machine_audio(machine: &mut PceMachine) -> Vec<f32> {
    let mut samples = Vec::new();
    machine.drain_audio_samples_into(&mut samples);
    samples
}

fn drain_psg_audio(psg: &mut HuC6280Psg) -> Vec<f32> {
    let mut samples = Vec::new();
    psg.drain_audio_samples_into(&mut samples);
    samples
}

fn advance_to_vce_line(machine: &mut PceMachine, line: u16) {
    while machine.vce_line_index() != line {
        machine.step_boundary().unwrap();
    }
}

#[test]
fn construction_uses_the_physical_reset_vector_and_reports_large_hucards() {
    let mut rom = rom_with_program(&[0xEA]);
    set_vector(&mut rom, 0x1FFE, 0xE123);
    let machine = PceMachine::new(rom).unwrap();
    assert_eq!(machine.cpu().cpu().registers().pc, 0xE123);
    assert_eq!(machine.cpu().cpu().speed_mode(), SpeedMode::Low);
    assert_eq!(machine.devices().psg().revision(), PsgRevision::HuC6280);
    assert_eq!(machine.master_ticks(), 0);

    assert!(matches!(
        PceMachine::new(vec![0; HUCARD_ROM_REGION_LEN + 1]),
        Err(PceMachineError::BusConstruction(_))
    ));
}

#[test]
fn machine_reset_preserves_populous_ram() {
    let mut rom = rom_with_program(&[0xA9, 0x5A, 0x8D, 0x00, 0x40]);
    rom.resize(super::POPULOUS_HUCARD_IMAGE_LEN, 0xEA);
    let descriptor =
        PceCartridgeDescriptor::default().with_hucard_board(super::PceHuCardBoard::Populous);
    let mut machine = PceMachine::with_cartridge(rom, descriptor).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0x40);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();
    assert_eq!(machine.hucard_ram().unwrap()[0], 0x5A);

    machine.reset();
    assert_eq!(machine.hucard_ram().unwrap()[0], 0x5A);
}

#[test]
fn machine_psg_revision_override_preserves_the_original_default() {
    let mut revised =
        PceMachine::with_psg_revision(rom_with_program(&[0xEA]), PsgRevision::HuC6280A).unwrap();
    assert_eq!(revised.devices().psg().revision(), PsgRevision::HuC6280A);
    revised.reset();
    assert_eq!(revised.devices().psg().revision(), PsgRevision::HuC6280A);
}

#[test]
fn read_only_debug_snapshot_exposes_cpu_mapping_and_side_effect_free_rom_peeks() {
    let machine = PceMachine::new(rom_with_program(&[0xEA, 0xD4])).unwrap();
    let snapshot = machine.debug_snapshot();

    assert_eq!(snapshot.registers().pc, RESET_PC);
    assert_eq!(snapshot.mapping_registers()[7], 0);
    assert_eq!(snapshot.physical_page(RESET_PC), 0);
    assert_eq!(snapshot.physical_address(RESET_PC), 0);
    assert_eq!(snapshot.physical_pc(), 0);
    assert_eq!(snapshot.speed_mode(), SpeedMode::Low);
    assert_eq!(snapshot.timer_counter(), 0);
    assert_eq!(snapshot.timer_reload(), 0);
    assert!(!snapshot.timer_running());
    assert_eq!(snapshot.timer_prescaler_ticks(), 0);
    assert_eq!(snapshot.irq_disable(), 0);
    assert_eq!(snapshot.irq_request(), 0);
    assert_eq!(snapshot.sampled_interrupt(), None);
    assert_eq!(snapshot.execution_state(), PceExecutionState::Running);
    assert_eq!(machine.debug_peek_cpu8(0), 0xEA);
    assert_eq!(machine.debug_peek_cpu8(1), 0xD4);
    assert_eq!(machine.rom_offset_for_cpu_address(RESET_PC), Some(0));
    assert_eq!(machine.master_ticks(), snapshot.master_ticks());
}

#[test]
fn debugger_controls_suspend_and_step_a_single_instruction() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA, 0xEA])).unwrap();

    machine.debug_suspend();
    let suspended = machine.run_until_frame().unwrap();
    assert_eq!(suspended.cpu_boundaries(), 0);
    assert_eq!(suspended.master_ticks(), 0);
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC);

    machine.debug_step();
    let stepped = machine.run_until_frame().unwrap();
    assert_eq!(stepped.cpu_boundaries(), 1);
    assert!(machine.is_cpu_suspended());
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC + 1);

    machine.debug_continue();
    assert_eq!(machine.execution_state(), PceExecutionState::Running);
}

#[test]
fn debugger_step_runs_pending_interrupt_then_one_instruction() {
    let mut rom = rom_with_program(&[0xEA, 0xEA]);
    rom[0x10] = 0xEA;
    set_vector(&mut rom, 0x1FFC, RESET_PC + 0x10);
    let mut machine = PceMachine::new(rom).unwrap();

    machine.cpu_mut().set_nmi_line(LineLevel::Low);
    machine.step_boundary().unwrap();
    machine.debug_suspend();
    machine.debug_step();

    let stepped = machine.run_until_frame().unwrap();
    assert_eq!(stepped.cpu_boundaries(), 2);
    assert!(machine.is_cpu_suspended());
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC + 0x11);
}

#[test]
fn guest_call_returns_to_the_suspended_huc6280_context() {
    let mut machine = PceMachine::new(rom_with_program(&[
        0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xA9, 0x42, 0x60,
    ]))
    .unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
    machine
        .cpu_mut()
        .cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    machine.debug_suspend();
    let before = machine.debug_snapshot();

    assert_eq!(machine.debug_execute_guest_call(RESET_PC + 6, 10), Ok(2));

    let after = machine.debug_snapshot();
    assert_eq!(after.registers().a, 0x42);
    assert_eq!(after.registers().pc, before.registers().pc);
    assert_eq!(after.registers().sp, before.registers().sp);
    assert!(!after.registers().status.contains(StatusFlags::INTERRUPT));
    assert_eq!(after.mapping_registers(), before.mapping_registers());
    assert!(machine.is_cpu_suspended());
    assert!(after.master_ticks() > before.master_ticks());
}

#[test]
fn guest_call_preserves_a_sampled_interrupt_until_return() {
    let mut rom = rom_with_program(&[0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xA9, 0x24, 0x60]);
    rom[0x10] = 0xEA;
    set_vector(&mut rom, 0x1FFC, RESET_PC + 0x10);
    let mut machine = PceMachine::new(rom).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
    machine.cpu_mut().set_nmi_line(LineLevel::Low);
    machine.step_boundary().unwrap();
    assert_eq!(
        machine.debug_snapshot().sampled_interrupt(),
        Some(InterruptSource::Nmi)
    );
    machine.debug_suspend();

    assert_eq!(machine.debug_execute_guest_call(RESET_PC + 6, 10), Ok(2));
    assert_eq!(
        machine.debug_snapshot().sampled_interrupt(),
        Some(InterruptSource::Nmi)
    );
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC + 1);
}

#[test]
fn logical_breakpoints_stop_before_execution_and_continue_past_the_hit() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA, 0xEA])).unwrap();
    machine.add_breakpoint(RESET_PC);
    machine.add_breakpoint(RESET_PC + 1);

    let first = machine.run_until_frame().unwrap();
    assert_eq!(first.cpu_boundaries(), 0);
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC);
    assert_eq!(machine.debug_hit_breakpoint(), Some(u32::from(RESET_PC)));

    machine.debug_continue();
    let second = machine.run_until_frame().unwrap();
    assert_eq!(second.cpu_boundaries(), 1);
    assert_eq!(machine.debug_snapshot().registers().pc, RESET_PC + 1);
    assert_eq!(
        machine.debug_hit_breakpoint(),
        Some(u32::from(RESET_PC + 1))
    );
}

#[test]
fn one_shot_and_hit_count_breakpoints_use_logical_cpu_addresses() {
    let mut one_shot = PceMachine::new(rom_with_program(&[0xEA])).unwrap();
    one_shot.add_one_shot_breakpoint(RESET_PC);
    one_shot.run_until_frame().unwrap();
    assert_eq!(one_shot.debug_hit_breakpoint(), Some(u32::from(RESET_PC)));
    assert!(one_shot.iter_breakpoints().next().is_none());
    assert!(one_shot.iter_one_shot_breakpoints().next().is_none());

    let mut counted = PceMachine::new(rom_with_program(&[0x80, 0xFE])).unwrap();
    counted.add_breakpoint_after(RESET_PC, 3);
    let run = counted.run_until_frame().unwrap();
    assert_eq!(run.cpu_boundaries(), 2);
    assert_eq!(counted.debug_hit_breakpoint(), Some(u32::from(RESET_PC)));
    let condition = counted.iter_breakpoint_hit_conditions().next().unwrap();
    assert_eq!(condition.target_hits, 3);
    assert_eq!(condition.hits, 3);
}

#[test]
fn logical_watchpoints_distinguish_mpr_aliases_and_suspend_after_access() {
    let mut machine = PceMachine::new(rom_with_program(&[
        0xA9, 0x5A, 0x8D, 0x00, 0x60, 0xA9, 0xA5, 0x8D, 0x00, 0x40,
    ]))
    .unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xF8);
    machine.cpu_mut().cpu_mut().set_mapping_register(3, 0xF8);
    machine.add_watchpoint_range(0x4000, 0x4000, WatchType::Write);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();
    assert!(machine.debug_hit_watchpoint().is_none());
    assert_eq!(machine.work_ram()[0], 0x5A);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();
    let hit = machine
        .debug_hit_watchpoint()
        .expect("logical write watchpoint should hit");
    assert!(machine.is_cpu_suspended());
    assert_eq!(hit.address, 0x4000);
    assert_eq!(hit.old_value, 0x5A);
    assert_eq!(hit.new_value, 0xA5);
}

#[test]
fn read_watchpoints_include_dummy_cpu_bus_cycles() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA, 0xEA])).unwrap();
    machine.add_watchpoint_range(RESET_PC + 1, RESET_PC + 1, WatchType::Read);

    let run = machine.run_until_frame().unwrap();
    assert_eq!(run.cpu_boundaries(), 1);
    let hit = machine
        .debug_hit_watchpoint()
        .expect("NOP dummy fetch should hit read watchpoint");
    assert_eq!(hit.address, u32::from(RESET_PC + 1));
    assert_eq!(hit.watch_type, WatchType::Read);
}

#[test]
fn write_watchpoint_old_value_peek_does_not_clear_vdc_status() {
    let mut machine = PceMachine::new(rom_with_program(&[0xA9, 0x00, 0x8D, 0x00, 0x40])).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xFF);
    machine
        .devices_mut()
        .vdc_mut()
        .latch_status(VdcStatus::RASTER_MATCH);
    machine.add_watchpoint_range(0x4000, 0x4000, WatchType::Write);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();

    assert!(
        machine
            .devices()
            .vdc()
            .status()
            .contains(VdcStatus::RASTER_MATCH)
    );
    let hit = machine
        .debug_hit_watchpoint()
        .expect("VDC write watchpoint should hit");
    assert_eq!(hit.old_value, 0xFF);
    assert_eq!(hit.new_value, 0x00);
}

#[test]
fn logical_debug_writes_route_on_chip_without_advancing_time_and_hit_watchpoints() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA])).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xFF);
    machine.add_watchpoint_range(0x4C00, 0x4C00, WatchType::Write);
    let ticks = machine.master_ticks();

    machine.debug_write_cpu8(0x4C00, 0x2A);

    assert_eq!(machine.master_ticks(), ticks);
    assert_eq!(machine.debug_snapshot().timer_reload(), 0x2A);
    assert!(machine.is_cpu_suspended());
    let hit = machine
        .debug_hit_watchpoint()
        .expect("debug timer write should hit watchpoint");
    assert_eq!(hit.address, 0x4C00);
    assert_eq!(hit.old_value, 0xFF);
    assert_eq!(hit.new_value, 0x2A);
}

#[test]
fn instruction_trace_records_exact_fetches_mapping_registers_and_logical_writes() {
    let mut machine = PceMachine::new(rom_with_program(&[0xA9, 0x5A, 0x8D, 0x00, 0x40])).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xF8);
    machine.set_instruction_trace_enabled(true);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();

    let entries = machine.instruction_trace().iter().collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].mode, TraceExecMode::HuC6280);
    assert_eq!(entries[0].pc, u32::from(RESET_PC));
    assert_eq!(entries[0].physical_rom_offset, Some(0));
    assert_eq!(entries[0].bank, Some(0));
    assert_eq!(entries[0].instruction_bytes(), &[0xA9, 0x5A]);
    assert_eq!(entries[0].register_deltas()[0].register, 0);
    assert_eq!(entries[0].register_deltas()[0].value, 0x5A);
    assert!(entries[0].cycle < entries[1].cycle);

    assert_eq!(entries[1].pc, u32::from(RESET_PC + 2));
    assert_eq!(entries[1].physical_rom_offset, Some(2));
    assert_eq!(entries[1].bank, Some(0));
    assert_eq!(entries[1].instruction_bytes(), &[0x8D, 0x00, 0x40]);
    assert_eq!(entries[1].writes().len(), 1);
    assert_eq!(entries[1].writes()[0].address, 0x4000);
    assert_eq!(entries[1].writes()[0].old_value, 0);
    assert_eq!(entries[1].writes()[0].new_value, 0x5A);
    assert_eq!(entries[1].writes()[0].width, TraceWriteWidth::Byte);
    assert_eq!(entries[1].writes()[0].kind, TraceWriteKind::Memory);
}

#[test]
fn instruction_trace_captures_mpr_changes_after_the_original_fetch_mapping() {
    let mut machine = PceMachine::new(rom_with_program(&[0xA9, 0x2A, 0x53, 0x04])).unwrap();
    machine.set_instruction_trace_enabled(true);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();

    let entry = machine.instruction_trace().iter().nth(1).unwrap();
    assert_eq!(entry.pc, u32::from(RESET_PC + 2));
    assert_eq!(entry.bank, Some(0));
    assert_eq!(entry.physical_rom_offset, Some(2));
    assert_eq!(entry.instruction_bytes(), &[0x53, 0x04]);
    assert!(
        entry
            .register_deltas()
            .iter()
            .any(|delta| delta.register == 8 && delta.value == 0x2A)
    );
}

#[test]
fn instruction_trace_records_direct_vdc_port_writes_as_io() {
    let mut machine = PceMachine::new(rom_with_program(&[0x03, 0x0C])).unwrap();
    machine.set_instruction_trace_enabled(true);

    machine.step_boundary().unwrap();

    let entry = machine.instruction_trace().iter().next().unwrap();
    assert_eq!(entry.instruction_bytes(), &[0x03, 0x0C]);
    assert_eq!(entry.writes().len(), 1);
    assert_eq!(entry.writes()[0].address, 0);
    assert_eq!(entry.writes()[0].old_value, 0xFF);
    assert_eq!(entry.writes()[0].new_value, 0x0C);
    assert_eq!(entry.writes()[0].kind, TraceWriteKind::Io);
}

#[test]
fn instruction_trace_does_not_change_machine_output() {
    let rom = rom_with_program(&[
        0xD4, 0xA9, 0xF8, 0x53, 0x02, 0xA9, 0x00, 0x8D, 0x00, 0x20, 0x1A, 0x80, 0xFA,
    ]);
    let mut untraced = PceMachine::new(rom.clone()).unwrap();
    let mut traced = PceMachine::new(rom).unwrap();
    traced.set_instruction_trace_enabled(true);

    for _ in 0..2 {
        untraced.run_until_frame().unwrap();
        traced.run_until_frame().unwrap();
    }

    assert_eq!(
        super::save_state::encode_state(&untraced).unwrap(),
        super::save_state::encode_state(&traced).unwrap()
    );
    assert_eq!(untraced.framebuffer(), traced.framebuffer());
    let mut untraced_audio = Vec::new();
    let mut traced_audio = Vec::new();
    untraced.drain_audio_samples_into(&mut untraced_audio);
    traced.drain_audio_samples_into(&mut traced_audio);
    assert_eq!(untraced_audio, traced_audio);
}

#[test]
fn interrupt_event_breakpoint_suspends_after_service_and_emits_trace_event() {
    let mut rom = rom_with_program(&[0xEA, 0xEA]);
    rom[0x10] = 0xEA;
    set_vector(&mut rom, 0x1FFC, RESET_PC + 0x10);
    let mut machine = PceMachine::new(rom).unwrap();
    machine.set_instruction_trace_enabled(true);
    machine.set_event_breakpoint(DebugEvent::Interrupt, true);
    machine.set_event_breakpoint(DebugEvent::Dma, true);
    assert_eq!(
        machine.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );

    machine.cpu_mut().set_nmi_line(LineLevel::Low);
    machine.step_boundary().unwrap();
    let run = machine.run_until_frame().unwrap();

    assert_eq!(run.cpu_boundaries(), 1);
    assert!(machine.is_cpu_suspended());
    assert_eq!(machine.debug_hit_event(), Some(DebugEvent::Interrupt));
    let entry = machine.instruction_trace().iter().last().unwrap();
    assert_eq!(entry.pc, u32::from(RESET_PC + 1));
    assert_eq!(entry.bank, Some(0));
    assert_eq!(entry.physical_rom_offset, Some(1));
    assert!(entry.instruction_bytes().is_empty());
    assert_eq!(entry.event, Some(DebugEvent::Interrupt));
    assert_eq!(machine.cpu().cpu().registers().pc, RESET_PC + 0x10);

    machine.debug_continue();
    assert_eq!(machine.debug_hit_event(), None);
    assert_eq!(machine.execution_state(), PceExecutionState::Running);
}

#[test]
fn opcode_history_records_instruction_fetch_mapping_and_keeps_recent_entries() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA, 0xD4, 0x54])).unwrap();
    machine.set_opcode_history_enabled(true);

    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();
    machine.step_boundary().unwrap();

    let history = machine.recent_opcodes(2);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].logical_pc(), RESET_PC + 2);
    assert_eq!(history[0].physical_pc(), 2);
    assert_eq!(history[0].opcode(), 0x54);
    assert!(history[0].master_ticks() > history[1].master_ticks());
    assert_eq!(history[1].logical_pc(), RESET_PC + 1);
    assert_eq!(history[1].opcode(), 0xD4);

    machine.reset();
    assert!(machine.recent_opcodes(32).is_empty());
}

#[test]
fn debug_rom_resolution_tracks_mprs_and_ignores_non_hucard_pages() {
    let mut rom = vec![0xEA; 0x60_000];
    rom[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    let mut machine = PceMachine::new(rom).unwrap();
    let initial_token = machine.rom_mapping_token();

    machine.cpu_mut().cpu_mut().set_mapping_register(3, 0x2A);
    let snapshot = machine.debug_snapshot();
    assert_eq!(snapshot.physical_page(0x6123), 0x2A);
    assert_eq!(snapshot.physical_address(0x6123), 0x054123);
    assert_eq!(machine.rom_offset_for_cpu_address(0x6123), Some(0x014123));
    assert_ne!(machine.rom_mapping_token(), initial_token);

    machine.cpu_mut().cpu_mut().set_mapping_register(3, 0xF8);
    assert_eq!(machine.rom_offset_for_cpu_address(0x6123), None);
}

#[test]
fn reset_presented_frame_is_empty_and_describes_fixed_storage() {
    let machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    let frame = machine.presented_frame();

    assert_eq!(
        frame.storage_dimensions(),
        (PCE_ACTIVE_FRAME_WIDTH, PCE_ACTIVE_FRAME_HEIGHT)
    );
    assert_eq!(frame.rgba().len(), PCE_ACTIVE_FRAME_RGBA_BYTES);
    assert_eq!(frame.rows().len(), PCE_ACTIVE_FRAME_HEIGHT);
    assert!(frame.rows().iter().all(|row| !row.is_active()));
    assert_eq!(frame.active_bounds(), None);
    assert_eq!(frame.signal_bounds().first_row(), PCE_SIGNAL_FIRST_ROW);
    assert_eq!(frame.signal_bounds().row_end(), PCE_SIGNAL_ROW_END);
    assert_eq!(frame.signal_bounds().height(), 242);
    assert!(
        frame
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == PCE_ACTIVE_FRAME_UNUSED_RGBA)
    );
}

#[test]
fn absolute_vce_rows_preserve_224_239_and_full_240_signal_placement() {
    for vce_control in [0, 0x04] {
        for (vertical_sync, vertical_display, vertical_end, expected) in [
            (0x1702, 0x00DF, 0x000A, (28, 252, 224)),
            (0x0F02, 0x00EF, 0x0004, (20, 260, 239)),
            (0x0E02, 0x00EF, 0x0004, (19, 259, 240)),
        ] {
            let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
            machine
                .devices_mut()
                .vce_mut()
                .write_port(VcePort::from_offset(0), vce_control);
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalSync,
                vertical_sync,
            );
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalDisplay,
                vertical_display,
            );
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalDisplayEnd,
                vertical_end,
            );

            machine.run_until_frame().unwrap();

            let frame = machine.presented_frame();
            let active = frame.active_bounds().unwrap();
            assert_eq!(
                (active.first_row(), active.row_end()),
                (expected.0, expected.1)
            );
            let signal = frame.signal_bounds();
            assert_eq!(
                (signal.first_row(), signal.row_end()),
                (PCE_SIGNAL_FIRST_ROW, PCE_SIGNAL_ROW_END)
            );
            let visible_start = active.first_row().max(signal.first_row());
            let visible_end = active.row_end().min(signal.row_end());
            assert_eq!(visible_end - visible_start, expected.2);
        }
    }
}

#[test]
fn presented_frame_reports_variable_active_widths_clocks_and_bounds() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 31);
    advance_to_vce_line(&mut machine, 4);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 63);
    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 1);
    machine.run_until_frame().unwrap();

    let frame = machine.presented_frame();
    assert_eq!(frame.rows()[3].active_width(), 256);
    assert_eq!(
        frame.rows()[3].pixel_clock(),
        Some(VcePixelClock::DivideByFour)
    );
    assert_eq!(frame.rows()[4].active_width(), 512);
    assert_eq!(
        frame.rows()[4].pixel_clock(),
        Some(VcePixelClock::DivideByThree)
    );
    let bounds = frame.active_bounds().unwrap();
    assert_eq!(bounds.first_row(), 3);
    assert_eq!(bounds.row_end(), 261);
    assert_eq!(bounds.height(), 258);
    assert_eq!(bounds.maximum_width(), 512);
}

#[test]
fn selected_vdc_pixel_clocks_preserve_fractional_master_ticks_across_modes() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();

    let csh = machine.step_boundary().unwrap();
    assert_eq!(csh.master_ticks(), 36);
    assert_eq!(machine.vdc_pixel_clock_remainder(), 0);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 1);

    let nop = machine.step_boundary().unwrap();
    assert_eq!(nop.master_ticks(), 6);
    assert_eq!(machine.vdc_pixel_clock_remainder(), 2);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 2);

    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 1);
    let branch = machine.step_boundary().unwrap();
    assert_eq!(branch.master_ticks(), 12);
    assert_eq!(machine.vdc_pixel_clock_remainder(), 2);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 2);

    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 2);
    let next_nop = machine.step_boundary().unwrap();
    assert_eq!(next_nop.master_ticks(), 6);
    assert_eq!(machine.vdc_pixel_clock_remainder(), 0);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 2);
    assert_eq!(machine.master_ticks(), 60);
}

#[test]
fn vram_dma_uses_exact_selected_pixel_slots_in_the_machine_clock_domain() {
    for (control, divisor) in [(0, 4_u64), (1, 3), (2, 2)] {
        let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
        machine
            .devices_mut()
            .vce_mut()
            .write_port(VcePort::from_offset(0), control);
        machine.devices_mut().vdc_mut().vram_mut()[0x0100] = 0xCAFE;
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 0);

        machine.advance_devices_for_test(3 * divisor).unwrap();
        assert_eq!(machine.devices().vdc().vram()[0x0200], 0, "mode={control}");
        machine.advance_devices_for_test(divisor).unwrap();
        assert_eq!(
            machine.devices().vdc().vram()[0x0200],
            0xCAFE,
            "mode={control}"
        );
    }
}

#[test]
fn satb_dma_completes_on_selected_clock_1024_not_1023_in_every_mode() {
    for (control, divisor) in [(0, 4_u64), (1, 3), (2, 2)] {
        let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
        machine
            .devices_mut()
            .vce_mut()
            .write_port(VcePort::from_offset(0), control);
        machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0200].fill(0x1234);
        write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);
        assert!(
            machine
                .devices_mut()
                .vdc_mut()
                .start_satb_dma_for_vertical_blank()
        );

        machine.advance_devices_for_test(1023 * divisor).unwrap();
        assert_eq!(
            machine
                .devices()
                .vdc()
                .active_satb_dma()
                .unwrap()
                .remaining_words(),
            1,
            "mode={control}"
        );
        machine.advance_devices_for_test(divisor).unwrap();
        assert_eq!(machine.devices().vdc().active_satb_dma(), None);
        assert_eq!(machine.devices().vdc().satb()[255], 0x1234);
    }
}

#[test]
fn final_cycle_video_writes_do_not_retroactively_change_earlier_instruction_time() {
    let mut hsr = PceMachine::new(rom_with_program(&[0x23, 0x7F])).unwrap();
    write_vdc_register(hsr.devices_mut(), VdcRegister::HorizontalDisplay, 0);
    write_vdc_register(hsr.devices_mut(), VdcRegister::HorizontalSync, 0);
    let step = hsr.step_boundary().unwrap();
    assert_eq!(step.wait_cycles(), 1);
    assert_eq!(
        hsr.devices().vdc().horizontal_phase(),
        super::VdcHorizontalPhase::ActiveDisplay
    );
    assert_eq!(hsr.devices().vdc().horizontal_phase_pixels_remaining(), 1);

    let mut vce = PceMachine::new(rom_with_program(&[0x8D, 0x00, 0x24])).unwrap();
    vce.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
    vce.cpu_mut().cpu_mut().registers_mut().a = 2;
    let step = vce.step_boundary().unwrap();
    assert_eq!(step.wait_cycles(), 1);
    assert_eq!(
        vce.devices().vce().pixel_clock(),
        VcePixelClock::DivideByTwo
    );
    assert_eq!(vce.devices().vdc().dma_pixel_remainder(), 2);
}

#[test]
fn only_high_byte_vram_cycles_add_dynamic_dma_contention_waits() {
    let mut machine = PceMachine::new(rom_with_program(&[0x13, 0x34, 0x23, 0x12])).unwrap();
    write_vdc_register(machine.devices_mut(), VdcRegister::VramData, 0);
    machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0120].fill(0xBEEF);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 31);
    let _ = machine
        .devices_mut()
        .vdc_mut()
        .write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);

    let low = machine.step_boundary().unwrap();
    assert_eq!(low.wait_cycles(), 1);
    assert_eq!(low.vram_contention_wait_cycles(), 0);
    assert!(machine.devices().vdc().active_vram_dma().is_some());

    let high = machine.step_boundary().unwrap();
    assert!(matches!(
        high.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0x23
    ));
    assert!(high.vram_contention_wait_cycles() != 0);
    assert_eq!(high.wait_cycles(), 1 + high.vram_contention_wait_cycles());
    assert_eq!(machine.devices().vdc().active_vram_dma(), None);
    assert_eq!(machine.devices().vdc().vram()[1], 0x1234);
    assert_eq!(
        high.master_ticks(),
        u64::from(4 + high.wait_cycles()) * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
}

#[test]
fn sync_output_change_does_not_interrupt_a_machine_line_boundary() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_timer(super::TimerPort::Control, 1);
    machine.advance_devices_for_test(1_360).unwrap();
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0010);

    let master_before = machine.master_ticks();
    assert_eq!(machine.advance_devices_for_test(12).unwrap(), (1, 0));

    assert_eq!(machine.master_ticks(), master_before + 12);
    assert_eq!(machine.vce_line_accumulator(), 7);
    assert_eq!(machine.cpu().on_chip_io().timer_prescaler_ticks(), 1_372);
    assert_eq!(machine.devices().psg().master_tick_remainder(), 4);
}

#[test]
fn marr_prefetch_and_vrr_refill_contend_but_buffer_and_register_accesses_do_not() {
    for (selected, program, mapped) in [
        (VdcRegister::MemoryAddressRead, vec![0x23, 0x01], false),
        (VdcRegister::VramData, vec![0xAD, 0x03, 0x20], true),
    ] {
        let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
        if mapped {
            machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
        }
        machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0120].fill(0xBEEF);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 31);
        let _ = machine
            .devices_mut()
            .vdc_mut()
            .write_port(VdcPort::SelectOrStatus, selected as u8);

        let step = machine.step_boundary().unwrap();
        assert!(
            step.vram_contention_wait_cycles() != 0,
            "selected={selected:?}"
        );
        assert_eq!(machine.devices().vdc().active_vram_dma(), None);
    }

    for (selected, program) in [
        (VdcRegister::VramData, vec![0xAD, 0x02, 0x20]),
        (VdcRegister::Control, vec![0x8D, 0x03, 0x20]),
    ] {
        let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
        machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0140].fill(0xBEEF);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 63);
        let _ = machine
            .devices_mut()
            .vdc_mut()
            .write_port(VdcPort::SelectOrStatus, selected as u8);

        let step = machine.step_boundary().unwrap();
        assert_eq!(step.wait_cycles(), 1, "selected={selected:?}");
        assert_eq!(
            step.vram_contention_wait_cycles(),
            0,
            "selected={selected:?}"
        );
        assert!(machine.devices().vdc().active_vram_dma().is_some());
    }
}

#[test]
fn presented_frame_stays_stable_while_the_back_frame_renders() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 31);
    machine.run_until_frame().unwrap();
    let before_rgba = machine.presented_frame().rgba().to_vec();
    let before_rows = machine.presented_frame().rows().to_vec();
    let before_bounds = machine.presented_frame().active_bounds();

    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 127);
    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 2);
    advance_to_vce_line(&mut machine, 5);

    let frame = machine.presented_frame();
    assert_eq!(frame.rgba(), before_rgba);
    assert_eq!(frame.rows().as_slice(), before_rows);
    assert_eq!(frame.active_bounds(), before_bounds);
}

#[test]
fn reserved_compatibility_nops_continue_machine_execution() {
    let mut machine = PceMachine::new(rom_with_program(&[0x0B])).unwrap();
    let step = machine.step_boundary().unwrap();
    assert!(matches!(
        step.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0x0B && cpu.cycles == 2
    ));
}

#[test]
fn pce_devices_route_controller_lines_with_base_console_upper_bits() {
    let mut bus = BaseBus::new(Vec::new(), PceDevices::new(ControllerPort::two_button())).unwrap();
    assert_eq!(bus.read(0x1F_F000), BASE_PCE_NO_CD_CONTROLLER_UPPER_BITS);
    bus.write(0x1F_F123, 0);
    assert_eq!(bus.read(0x1F_F3FF), 0xFF);

    let mut disconnected = BaseBus::new(Vec::new(), PceDevices::default()).unwrap();
    assert_eq!(disconnected.read(0x1F_F000), 0xFF);
}

#[test]
fn cartridge_descriptor_selects_console_wiring_with_explicit_override() {
    let turbografx16_hashes = [
        [
            0x90, 0xE0, 0x4D, 0x9F, 0xCD, 0x0A, 0x57, 0xAD, 0x07, 0xBA, 0x99, 0x53, 0x52, 0xF0,
            0x06, 0x1E, 0x39, 0x6E, 0x8A, 0x51, 0xC4, 0x70, 0xE1, 0xF1, 0x64, 0x73, 0x9F, 0xB4,
            0xB8, 0x61, 0x39, 0xCA,
        ],
        [
            0xC5, 0xA3, 0x9C, 0x9D, 0x9B, 0x2D, 0x75, 0x32, 0x44, 0x81, 0x6E, 0xAF, 0xD6, 0x8F,
            0x50, 0x4A, 0x85, 0x59, 0x08, 0xEE, 0xBA, 0xB1, 0xB1, 0xC8, 0xFE, 0xA2, 0xBB, 0xF7,
            0xA4, 0xA8, 0x13, 0xC7,
        ],
    ];
    for hash in turbografx16_hashes {
        let descriptor = PceCartridgeDescriptor::from_sha256(hash);
        assert_eq!(descriptor.console_wiring(), PceConsoleWiring::TurboGrafx16);
        assert_eq!(
            descriptor
                .with_console_wiring(PceConsoleWiring::PcEngine)
                .console_wiring(),
            PceConsoleWiring::PcEngine
        );
    }
    assert_eq!(
        PceCartridgeDescriptor::from_sha256([0; 32]).console_wiring(),
        PceConsoleWiring::PcEngine
    );

    let descriptor = PceCartridgeDescriptor::from_sha256(turbografx16_hashes[0]);
    let machine = PceMachine::with_cartridge(rom_with_program(&[0xEA]), descriptor).unwrap();
    assert_eq!(
        machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    let mut bus = BaseBus::new(
        Vec::new(),
        PceDevices::with_console_wiring(
            ControllerPort::two_button(),
            PceConsoleWiring::TurboGrafx16,
        ),
    )
    .unwrap();
    assert_eq!(
        bus.read(0x1F_F000),
        BASE_TURBOGRAFX16_NO_CD_CONTROLLER_UPPER_BITS
    );
}

#[test]
fn known_supergrafx_only_hucards_construct_the_supergrafx_machine() {
    for hash in [
        "5006f2da9cb645312a0c589044df50d3f97106d2d2291bf9883dacf98960c2fe",
        "5f3b430e34c79218a9f89a403a286037b2fb172b528373df5ba70aedbecd36d7",
        "41e06beeacfd05c837c9bb76da73c28d14dc2f66250a245b6931712f36c4e457",
        "482fff401f8a0f4248af16224c31bc166a583b491413559a89c425165420a9dd",
        "9b57cdf0d0b110f4128b863419d5be99a3708bfb11cfbe1696f25449b991026d",
    ] {
        let descriptor = PceCartridgeDescriptor::from_sha256(sha256(hash));
        assert_eq!(
            descriptor.required_hardware(),
            PceCartridgeHardware::SuperGrafx
        );
        let machine = PceMachine::with_cartridge(rom_with_program(&[0xEA]), descriptor).unwrap();
        assert_eq!(
            machine.hardware_topology(),
            super::PceHardwareTopology::SuperGrafx
        );
        assert_eq!(machine.devices().psg().revision(), PsgRevision::HuC6280A);
    }

    let base = PceCartridgeDescriptor::from_sha256([0; 32]);
    assert_eq!(base.required_hardware(), PceCartridgeHardware::Base);
    assert!(PceMachine::with_cartridge(rom_with_program(&[0xEA]), base).is_ok());
}

#[test]
fn csh_and_csl_charge_all_cycles_at_the_entering_speed() {
    let mut machine = PceMachine::new(rom_with_program(&[0xD4, 0xEA, 0x54, 0xEA])).unwrap();
    let csh = machine.step_boundary().unwrap();
    let high_nop = machine.step_boundary().unwrap();
    let csl = machine.step_boundary().unwrap();
    let low_nop = machine.step_boundary().unwrap();

    assert!(matches!(
        csh.action(),
        PceCpuAction::Instruction(step) if step.opcode == 0xD4
    ));
    assert_eq!(csh.entering_speed(), SpeedMode::Low);
    assert_eq!(
        csh.master_ticks(),
        3 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(high_nop.entering_speed(), SpeedMode::High);
    assert_eq!(
        high_nop.master_ticks(),
        2 * PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(csl.entering_speed(), SpeedMode::High);
    assert_eq!(
        csl.master_ticks(),
        3 * PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(low_nop.entering_speed(), SpeedMode::Low);
    assert_eq!(
        low_nop.master_ticks(),
        2 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(machine.master_ticks(), 75);
}

#[test]
fn vdc_vce_wait_aperture_includes_unavailable_mirrors_and_excludes_neighbors() {
    for (mapping, address, expected_waits) in [
        (0xFE, 0x3FFF, 0),
        (0xFF, 0x2000, 1),
        (0xFF, 0x2001, 1),
        (0xFF, 0x23FF, 1),
        (0xFF, 0x2400, 1),
        (0xFF, 0x27FF, 1),
        (0xFF, 0x2800, 0),
    ] {
        let [low, high] = (address as u16).to_le_bytes();
        let mut machine = PceMachine::new(rom_with_program(&[0xAD, low, high])).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(1, mapping);
        let step = machine.step_boundary().unwrap();
        assert!(matches!(
            step.action(),
            PceCpuAction::Instruction(cpu) if cpu.opcode == 0xAD && cpu.cycles == 5
        ));
        assert_eq!(step.wait_cycles(), expected_waits, "address={address:04x}");
        assert_eq!(
            step.master_ticks(),
            u64::from(5 + expected_waits) * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
        );
    }
}

#[test]
fn vce_opcode_and_dummy_fetches_each_add_a_wait_cycle() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA])).unwrap();
    let vce = machine.devices_mut().vce_mut();
    vce.write_port(VcePort::from_offset(2), 0);
    vce.write_port(VcePort::from_offset(3), 0);
    vce.write_port(VcePort::from_offset(4), 0xEA);
    vce.write_port(VcePort::from_offset(5), 0);
    vce.write_port(VcePort::from_offset(2), 0);
    vce.write_port(VcePort::from_offset(3), 0);
    machine.cpu_mut().cpu_mut().set_mapping_register(7, 0xFF);
    machine.cpu_mut().cpu_mut().registers_mut().pc = 0xE404;

    let step = machine.step_boundary().unwrap();
    assert!(matches!(
        step.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0xEA && cpu.cycles == 2
    ));
    assert_eq!(step.wait_cycles(), 2);
    assert_eq!(
        step.master_ticks(),
        4 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
}

#[test]
fn direct_vdc_stores_and_mapped_rmw_count_video_waits_without_changing_cpu_cycles() {
    let mut direct =
        PceMachine::new(rom_with_program(&[0x03, 0x05, 0x13, 0x34, 0x23, 0x12])).unwrap();
    for opcode in [0x03, 0x13, 0x23] {
        let step = direct.step_boundary().unwrap();
        assert!(matches!(
            step.action(),
            PceCpuAction::Instruction(cpu) if cpu.opcode == opcode && cpu.cycles == 4
        ));
        assert_eq!(step.wait_cycles(), PCE_VDC_VCE_ACCESS_WAIT_CYCLES);
        assert_eq!(
            step.master_ticks(),
            5 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
        );
    }
    assert_eq!(
        direct.devices().vdc().register(VdcRegister::Control),
        0x1234
    );

    let mut rmw = PceMachine::new(rom_with_program(&[0xE6, 0x00])).unwrap();
    rmw.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
    let step = rmw.step_boundary().unwrap();
    assert!(matches!(
        step.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0xE6 && cpu.cycles == 6
    ));
    assert_eq!(step.wait_cycles(), 2);
    assert_eq!(
        step.master_ticks(),
        8 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
}

#[test]
fn block_transfer_counts_only_words_inside_the_video_wait_aperture() {
    let program = [0x73, 0xFE, 0x47, 0x00, 0x50, 0x04, 0x00];
    let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xFF);

    let step = machine.step_boundary().unwrap();
    assert!(matches!(
        step.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0x73 && cpu.cycles == 41
    ));
    assert_eq!(step.wait_cycles(), 2);
    assert_eq!(
        step.master_ticks(),
        43 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
}

#[test]
fn video_waits_use_the_speed_at_cpu_action_entry() {
    for (speed, ticks_per_cycle) in [
        (
            SpeedMode::Low,
            PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        ),
        (
            SpeedMode::High,
            PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        ),
    ] {
        let mut machine = PceMachine::new(rom_with_program(&[0xAD, 0x00, 0x24])).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
        machine.cpu_mut().cpu_mut().set_speed_mode(speed);
        let step = machine.step_boundary().unwrap();
        assert_eq!(step.entering_speed(), speed);
        assert_eq!(step.wait_cycles(), 1);
        assert_eq!(step.master_ticks(), 6 * ticks_per_cycle);
        assert_eq!(machine.master_ticks(), 6 * ticks_per_cycle);
        assert_eq!(machine.vce_line_accumulator(), 6 * ticks_per_cycle);
    }
}

#[test]
fn psg_write_takes_effect_after_its_cpu_bus_cycle() {
    let mut program = vec![0xEA; 128];
    program.extend_from_slice(&[0xA9, 0x1F, 0x8D, 0x06, 0x28]);
    program.extend(std::iter::repeat_n(0xEA, 128));
    let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
    machine.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_dda(machine.devices_mut().psg_mut(), 0);

    let mut reference = HuC6280Psg::new();
    reference.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_dda(&mut reference, 0);

    for _ in 0..129 {
        let step = machine.step_boundary().unwrap();
        reference.advance_master_ticks(step.master_ticks());
    }
    let before = drain_machine_audio(&mut machine);
    assert_eq!(before, drain_psg_audio(&mut reference));
    assert!(before.iter().all(|sample| *sample == 0.0));

    let psg_clock_before = machine.devices().psg().resampler_clock();
    let store = machine.step_boundary().unwrap();
    assert!(matches!(
        store.action(),
        PceCpuAction::Instruction(step) if step.opcode == 0x8D && step.cycles == 5
    ));
    assert_eq!(
        machine.devices().psg().resampler_clock(),
        psg_clock_before + (store.master_ticks() / PSG_INTERNAL_MASTER_CLOCK_DIVISOR) as u32
    );
    assert_ne!(machine.devices().psg().resampler_levels(), (0, 0));
    reference.advance_master_ticks(store.master_ticks());
    reference.write_port(psg_port(6), 0x1F);
    assert_eq!(
        drain_machine_audio(&mut machine),
        drain_psg_audio(&mut reference)
    );

    for _ in 0..128 {
        let step = machine.step_boundary().unwrap();
        reference.advance_master_ticks(step.master_ticks());
    }
    let after = drain_machine_audio(&mut machine);
    assert_eq!(after, drain_psg_audio(&mut reference));
    assert!(after.iter().any(|sample| *sample > 0.01));
    assert_eq!(machine.master_ticks(), (128 * 2 + 2 + 5 + 128 * 2) * 12);
}

#[test]
fn high_speed_psg_writes_refresh_after_the_final_bus_cycle_for_both_master_phases() {
    for initial_remainder in [0_u64, 3] {
        let mut machine = PceMachine::new(rom_with_program(&[0x8D, 0x06, 0x28])).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
        machine.cpu_mut().cpu_mut().set_speed_mode(SpeedMode::High);
        machine.cpu_mut().cpu_mut().registers_mut().a = 31;
        configure_dda(machine.devices_mut().psg_mut(), 0);
        let psg = machine.devices_mut().psg_mut();
        psg.set_sample_generation_enabled(false);
        psg.advance_master_ticks(
            u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS)
                * PSG_INTERNAL_MASTER_CLOCK_DIVISOR
                * 2,
        );
        psg.set_sample_generation_enabled(true);
        psg.advance_master_ticks(initial_remainder);

        let step = machine.step_boundary().unwrap();
        assert!(matches!(
            step.action(),
            PceCpuAction::Instruction(cpu) if cpu.opcode == 0x8D && cpu.cycles == 5
        ));
        assert_eq!(step.entering_speed(), SpeedMode::High);
        assert_eq!(step.master_ticks(), 15);
        assert_eq!(
            machine.devices().psg().master_tick_remainder(),
            ((initial_remainder + 15) % PSG_MASTER_CLOCK_DIVISOR) as u8
        );
        assert_eq!(
            machine.devices().psg().resampler_clock(),
            ((initial_remainder + 15) / PSG_INTERNAL_MASTER_CLOCK_DIVISOR) as u32
        );
        assert_ne!(machine.devices().psg().resampler_levels(), (0, 0));
    }
}

#[test]
fn machine_gain_scan_starts_after_the_psg_write_at_both_cpu_speeds() {
    for (speed, initial_remainder, expected_scan_clocks) in [
        (SpeedMode::Low, 3_u64, 8_u16),
        (SpeedMode::High, 0, 2),
        (SpeedMode::High, 3, 2),
    ] {
        let mut machine = PceMachine::new(rom_with_program(&[0x8D, 0x01, 0x28, 0xEA])).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
        machine.cpu_mut().cpu_mut().set_speed_mode(speed);
        machine.cpu_mut().cpu_mut().registers_mut().a = 0xFF;
        machine
            .devices_mut()
            .psg_mut()
            .advance_master_ticks(initial_remainder);

        let store = machine.step_boundary().unwrap();
        assert!(matches!(
            store.action(),
            PceCpuAction::Instruction(cpu) if cpu.opcode == 0x8D && cpu.cycles == 5
        ));
        assert_eq!(machine.devices().psg().gain_scan_state(), (true, false, 0));
        assert_eq!(
            machine.devices().psg().channels()[0].effective_right_attenuation(),
            31
        );

        machine.step_boundary().unwrap();
        assert_eq!(
            machine.devices().psg().gain_scan_state(),
            (true, false, expected_scan_clocks)
        );
        assert_eq!(
            machine.devices().psg().channels()[0].effective_right_attenuation(),
            31
        );
    }
}

#[test]
fn psg_cycle_accounting_covers_direct_internal_dummy_idle_and_speed_paths() {
    let program = [
        0x03, 0x00, 0xA9, 0x7F, 0x8D, 0x00, 0x2C, 0xD4, 0xEA, 0x54, 0xEA,
    ];
    let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
    machine.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(machine.devices_mut().psg_mut());
    let mut reference = HuC6280Psg::new();
    reference.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(&mut reference);

    let mut elapsed = 0;
    for _ in 0..7 {
        let step = machine.step_boundary().unwrap();
        elapsed += step.master_ticks();
        reference.advance_master_ticks(step.master_ticks());
    }

    assert_eq!(machine.master_ticks(), elapsed);
    assert_eq!(
        drain_machine_audio(&mut machine),
        drain_psg_audio(&mut reference)
    );
}

#[test]
fn interrupt_entry_advances_psg_once_per_reported_cycle() {
    let mut rom = rom_with_program(&[0x58, 0xEA]);
    set_vector(&mut rom, 0x1FF8, 0xE100);
    let mut machine = PceMachine::new(rom).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xF8);
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xFF);
    machine.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(machine.devices_mut().psg_mut());
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0004);
    machine
        .devices_mut()
        .vdc_mut()
        .latch_status(VdcStatus::RASTER_MATCH);
    let mut reference = HuC6280Psg::new();
    reference.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(&mut reference);

    let cli = machine.step_boundary().unwrap();
    reference.advance_master_ticks(cli.master_ticks());
    let delayed = machine.step_boundary().unwrap();
    reference.advance_master_ticks(delayed.master_ticks());
    let interrupt = machine.step_boundary().unwrap();
    reference.advance_master_ticks(interrupt.master_ticks());

    assert!(matches!(
        interrupt.action(),
        PceCpuAction::Interrupt(step) if step.source == InterruptSource::Irq1 && step.cycles == 8
    ));
    assert_eq!(interrupt.wait_cycles(), 3);
    assert_eq!(
        interrupt.master_ticks(),
        11 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(
        drain_machine_audio(&mut machine),
        drain_psg_audio(&mut reference)
    );
}

#[test]
fn trapped_video_opcode_fetch_commits_its_wait_before_faulting() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA])).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(7, 0xFF);
    machine.cpu_mut().cpu_mut().registers_mut().pc = 0xE000;

    assert_eq!(
        machine.force_unsupported_opcode_trap_after_fetch(),
        Err(PceMachineError::CpuTrap(CpuTrap::UnsupportedOpcode {
            pc: 0xE000,
            opcode: 0,
        }))
    );
    assert_eq!(
        machine.master_ticks(),
        2 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert_eq!(
        machine.vce_line_accumulator(),
        2 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE
    );
    assert!(machine.faulted());
    assert_eq!(
        machine.step_boundary(),
        Err(PceMachineError::FaultedUntilReset)
    );
}

#[test]
fn cpu_trap_commits_the_fetched_cycle_across_machine_time() {
    let mut machine = PceMachine::new(rom_with_program(&[0xEA])).unwrap();
    machine.cpu_mut().cpu_mut().set_speed_mode(SpeedMode::High);
    for _ in 0..227 {
        machine.step_boundary().unwrap();
    }
    assert_eq!(machine.master_ticks(), 1_362);
    assert_eq!(machine.vce_line_accumulator(), 1_362);
    assert_eq!(machine.vce_line_index(), 0);

    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_timer(super::TimerPort::CounterReload, 0x7F);
    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_timer(super::TimerPort::Control, 1);

    let psg = machine.devices_mut().psg_mut();
    psg.write_port(psg_port(0), 0);
    psg.write_port(psg_port(2), 1);
    for _ in 0..32 {
        psg.write_port(psg_port(6), 0);
    }
    psg.write_port(psg_port(4), 0x9F);
    psg.advance_master_ticks(3);
    assert_eq!(psg.channels()[0].wave_index(), 0);

    let trap = CpuTrap::UnsupportedOpcode {
        pc: RESET_PC + 227,
        opcode: 0xEA,
    };
    assert_eq!(
        machine.force_unsupported_opcode_trap_after_fetch(),
        Err(PceMachineError::CpuTrap(trap))
    );
    assert_eq!(machine.master_ticks(), 1_365);
    assert_eq!(machine.vce_line_accumulator(), 0);
    assert_eq!(machine.vce_line_index(), 1);
    assert_eq!(machine.cpu().on_chip_io().timer_prescaler_ticks(), 3);
    assert_eq!(machine.devices().psg().channels()[0].wave_index(), 1);
    assert_eq!(machine.cpu().cpu().registers().pc, RESET_PC + 228);
    assert!(machine.faulted());

    assert_eq!(
        machine.step_boundary(),
        Err(PceMachineError::FaultedUntilReset)
    );
    assert_eq!(machine.master_ticks(), 1_365);
    assert_eq!(machine.vce_line_accumulator(), 0);
    assert_eq!(machine.cpu().on_chip_io().timer_prescaler_ticks(), 3);
    assert_eq!(machine.devices().psg().channels()[0].wave_index(), 1);

    machine.reset();
    assert!(!machine.faulted());
    assert_eq!(machine.master_ticks(), 0);
    assert_eq!(machine.vce_line_accumulator(), 0);
    assert_eq!(machine.cpu().on_chip_io().timer_prescaler_ticks(), 0);
    machine.step_boundary().unwrap();
}

#[test]
fn maximum_sample_rate_drains_a_complete_machine_frame() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    machine.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(machine.devices_mut().psg_mut());
    let mut reference = HuC6280Psg::new();
    reference.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(&mut reference);

    let run = machine.run_until_frame().unwrap();
    reference.advance_master_ticks(run.master_ticks());
    let samples = drain_machine_audio(&mut machine);

    assert_eq!(samples, drain_psg_audio(&mut reference));
    assert!(
        (samples.len() / 2).abs_diff(expected_audio_frames(
            run.master_ticks(),
            MAX_PSG_SAMPLE_RATE
        )) <= 1
    );
}

#[test]
fn cpu_time_crosses_the_on_chip_timer_cadence() {
    let mut machine = PceMachine::new(rom_with_program(&[])).unwrap();
    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_timer(super::TimerPort::CounterReload, 0);
    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_timer(super::TimerPort::Control, 1);

    for _ in 0..127 {
        machine.step_boundary().unwrap();
    }
    assert!(!machine.cpu().on_chip_io().timer_irq_pending());
    assert_eq!(machine.master_ticks(), 3_048);
    machine.step_boundary().unwrap();
    assert!(machine.cpu().on_chip_io().timer_irq_pending());
    assert_eq!(machine.master_ticks(), 3_072);
}

#[test]
fn fixed_vce_cadence_publishes_after_exactly_262_lines() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    let run = machine.run_until_frame().unwrap();
    let frame_ticks =
        PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE * u64::from(VceFrameLength::Lines262.scanlines());

    assert_eq!(run.master_ticks(), frame_ticks);
    assert_eq!(run.frames_published(), 1);
    assert_eq!(machine.master_ticks(), frame_ticks);
    assert_eq!(machine.vce_line_accumulator(), 0);
    assert_eq!(machine.vce_line_index(), 0);
    assert_eq!(machine.vce_frame_length(), VceFrameLength::Lines262);
}

#[test]
fn provisional_ex11_machine_policy_publishes_exact_262_and_263_line_frames() {
    const { assert!(PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC) };
    for (frame_length, vce_control) in [
        (VceFrameLength::Lines262, 0),
        (VceFrameLength::Lines263, 0x04),
    ] {
        let mut rendered = Vec::new();
        for control in [0x0000, 0x0030] {
            let mut machine = PceMachine::new(nonblack_video_rom()).unwrap();
            write_vdc_register(machine.devices_mut(), VdcRegister::Control, control);
            machine
                .devices_mut()
                .vce_mut()
                .write_port(VcePort::from_offset(0), vce_control);

            let run = machine.run_until_frame().unwrap();
            let frame_ticks =
                PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE * u64::from(frame_length.scanlines());
            assert!(run.master_ticks() >= frame_ticks);
            assert!(run.master_ticks() < frame_ticks + 12);
            assert_eq!(run.frames_published(), 1);
            assert_eq!(machine.vce_frame_length(), frame_length);
            let first = usize::from(
                machine
                    .presented_frame()
                    .active_bounds()
                    .unwrap()
                    .first_row(),
            ) * PCE_ACTIVE_FRAME_WIDTH
                * 4;
            assert_eq!(
                &machine.framebuffer()[first..first + 4],
                &[0xFF, 0, 0, 0xFF]
            );
            rendered.push(machine.framebuffer().to_vec());
        }
        assert_eq!(rendered[0], rendered[1]);
    }
}

#[test]
fn magical_chase_cr_switch_continues_at_the_next_vce_boundary() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    advance_to_vce_line(&mut machine, 145);
    let vdc = machine.devices_mut().vdc_mut();
    vdc.write_port(VdcPort::SelectOrStatus, VdcRegister::Control as u8);
    vdc.write_port(VdcPort::DataLow, 0xFF);
    vdc.write_port(VdcPort::DataHigh, 0x0A);
    assert_eq!(vdc.register(VdcRegister::Control), 0x0AFF);
    assert!(vdc.sync_output().horizontal());
    assert!(vdc.sync_output().vertical());

    let run = machine.run_until_frame().unwrap();
    assert_eq!(run.frames_published(), 1);
    assert!(!machine.faulted());
    assert_eq!(machine.vce_line_index(), 0);
}

#[test]
fn nominal_264_line_vdc_profile_runs_in_a_262_line_vce_frame() {
    const { assert!(PROVISIONAL_PCE_VSYNC_ASSERT_NORMALIZED_TO_LINE_ZERO) };
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_1943_profile(machine.devices_mut());
    machine.run_until_frame().unwrap();
    assert_eq!(machine.vce_frame_length(), VceFrameLength::Lines262);
    assert!(
        machine
            .devices()
            .vdc()
            .status()
            .contains(VdcStatus::VERTICAL_BLANK)
    );
}

#[test]
fn cpu_program_configures_external_video_and_publishes_nonblack_backdrop() {
    let mut machine = PceMachine::new(nonblack_video_rom()).unwrap();
    machine.run_until_frame().unwrap();

    let first = usize::from(
        machine
            .presented_frame()
            .active_bounds()
            .unwrap()
            .first_row(),
    ) * PCE_ACTIVE_FRAME_WIDTH
        * 4;
    assert_eq!(
        &machine.framebuffer()[first..first + 4],
        &[0xFF, 0, 0, 0xFF]
    );
    let published = machine.framebuffer().to_vec();
    machine.step_boundary().unwrap();
    assert_eq!(machine.framebuffer(), published);
}

#[test]
fn vdc_irq1_is_sampled_after_the_instruction_following_cli() {
    let mut rom = rom_with_program(&[0x58, 0xEA]);
    set_vector(&mut rom, 0x1FF8, 0xE100);
    let mut machine = PceMachine::new(rom).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xF8);
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0004);
    machine
        .devices_mut()
        .vdc_mut()
        .latch_status(VdcStatus::RASTER_MATCH);

    assert!(matches!(
        machine.step_boundary().unwrap().action(),
        PceCpuAction::Instruction(step) if step.opcode == 0x58
    ));
    assert!(matches!(
        machine.step_boundary().unwrap().action(),
        PceCpuAction::Instruction(step) if step.opcode == 0xEA
    ));
    let interrupt = machine.step_boundary().unwrap();
    assert!(matches!(
        interrupt.action(),
        PceCpuAction::Interrupt(step) if step.source == InterruptSource::Irq1 && step.cycles == 8
    ));
    assert_eq!(machine.cpu().cpu().registers().pc, 0xE100);
    assert!(
        machine
            .devices()
            .vdc()
            .status()
            .contains(VdcStatus::RASTER_MATCH)
    );
}

#[test]
fn timer_expiry_on_the_instruction_boundary_is_serviced_at_both_speeds() {
    for (speed, ticks_per_cycle) in [
        (
            SpeedMode::Low,
            PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        ),
        (
            SpeedMode::High,
            PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        ),
    ] {
        let mut rom = rom_with_program(&[0xEA]);
        set_vector(&mut rom, 0x1FFA, 0xE100);
        let mut machine = PceMachine::new(rom).unwrap();
        machine.cpu_mut().cpu_mut().set_speed_mode(speed);
        machine
            .cpu_mut()
            .cpu_mut()
            .registers_mut()
            .status
            .remove(StatusFlags::INTERRUPT);
        machine
            .cpu_mut()
            .on_chip_io_mut()
            .write_timer(super::TimerPort::CounterReload, 0);
        machine
            .cpu_mut()
            .on_chip_io_mut()
            .write_timer(super::TimerPort::Control, 1);
        machine
            .cpu_mut()
            .advance_master_ticks(TIMER_MASTER_TICKS - 2 * ticks_per_cycle);

        let nop = machine.step_boundary().unwrap();
        assert!(matches!(
            nop.action(),
            PceCpuAction::Instruction(step) if step.opcode == 0xEA
        ));
        assert!(machine.cpu().on_chip_io().timer_irq_pending());
        assert_eq!(
            machine.cpu().sampled_interrupt(),
            Some(InterruptSource::Timer)
        );
        let interrupt = machine.step_boundary().unwrap();
        assert!(matches!(
            interrupt.action(),
            PceCpuAction::Interrupt(step) if step.source == InterruptSource::Timer
        ));
        assert_eq!(interrupt.entering_speed(), speed);
    }
}

#[test]
fn block_transfer_defers_a_pending_irq_until_its_completion_poll() {
    let program = [0x73, 0x00, 0x01, 0x00, 0x02, 0x01, 0x00];
    let mut rom = rom_with_program(&program);
    set_vector(&mut rom, 0x1FF8, 0xE100);
    let mut machine = PceMachine::new(rom).unwrap();
    machine
        .cpu_mut()
        .cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0004);
    machine
        .devices_mut()
        .vdc_mut()
        .latch_status(VdcStatus::RASTER_MATCH);

    let transfer = machine.step_boundary().unwrap();
    assert!(matches!(
        transfer.action(),
        PceCpuAction::Instruction(step) if step.opcode == 0x73 && step.cycles == 23
    ));
    assert_eq!(
        machine.cpu().sampled_interrupt(),
        Some(InterruptSource::Irq1)
    );
    assert!(matches!(
        machine.step_boundary().unwrap().action(),
        PceCpuAction::Interrupt(step) if step.source == InterruptSource::Irq1
    ));
}

#[test]
fn machine_services_pending_vram_dma_on_scheduled_pixel_slots() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    machine.set_instruction_trace_enabled(true);
    machine.set_event_breakpoint(DebugEvent::Dma, true);
    configure_external_262(machine.devices_mut(), 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 3);
    machine.run_until_frame().unwrap();
    assert!(machine.is_cpu_suspended());
    assert_eq!(machine.debug_hit_event(), Some(DebugEvent::Dma));
    assert_eq!(
        machine.instruction_trace().iter().last().unwrap().event,
        Some(DebugEvent::Dma)
    );
    assert_eq!(machine.devices().vdc().pending_vram_dma(), None);
    assert!(machine.devices().vdc().active_vram_dma().is_none());
    assert_eq!(machine.devices().vdc().vram()[0x0200..=0x0203], [0; 4]);
}

#[test]
fn machine_services_pending_satb_dma_at_vblank_in_exactly_256_slots() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    for index in 0..VDC_SATB_WORDS {
        machine.devices_mut().vdc_mut().vram_mut()[0x0100 + index] = 0x4000 | index as u16;
    }
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0010);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);
    assert!(machine.devices().vdc().pending_satb_dma().is_some());

    machine.run_until_frame().unwrap();
    advance_until_satb_dma_finishes(&mut machine);

    assert_eq!(machine.devices().vdc().pending_satb_dma(), None);
    assert_eq!(machine.devices().vdc().active_satb_dma(), None);
    assert_eq!(
        machine.devices().vdc().satb().as_slice(),
        &(0..VDC_SATB_WORDS)
            .map(|index| 0x4000 | index as u16)
            .collect::<Vec<_>>()
    );
    assert_eq!(machine.devices().vdc().status(), VdcStatus::empty());
}

#[test]
fn machine_satb_completion_respects_irq_gate_then_services_vram_dma() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    machine.devices_mut().vdc_mut().vram_mut()[0x0100] = 0xCAFE;
    machine.devices_mut().vdc_mut().vram_mut()[0x0200] = 0xBEEF;
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0010);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);

    machine.run_until_frame().unwrap();

    assert_eq!(machine.devices().vdc().pending_vram_dma(), None);
    assert_eq!(machine.devices().vdc().active_vram_dma(), None);
    assert_eq!(machine.devices().vdc().vram()[0x0200], 0xCAFE);
    assert_eq!(machine.devices().vdc().status(), VdcStatus::empty());
    assert_eq!(machine.devices().vdc().irq_level(), LineLevel::High);

    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0011);
    machine.run_until_frame().unwrap();
    assert!(
        machine
            .devices()
            .vdc()
            .status()
            .contains(VdcStatus::SATB_DMA_COMPLETE)
    );
    assert_eq!(machine.devices().vdc().irq_level(), LineLevel::Low);

    let status = machine
        .devices_mut()
        .vdc_mut()
        .read_port(VdcPort::SelectOrStatus);
    assert_eq!(status & VdcStatus::SATB_DMA_COMPLETE.bits(), 0x08);
    assert_eq!(machine.devices().vdc().irq_level(), LineLevel::High);
}

#[test]
fn machine_repeats_satb_copy_at_next_vblank_after_source_change() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    for index in 0..VDC_SATB_WORDS {
        machine.devices_mut().vdc_mut().vram_mut()[0x0100 + index] = 0x1000 | index as u16;
        machine.devices_mut().vdc_mut().vram_mut()[0x0200 + index] = 0x2000 | index as u16;
    }
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0010);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);
    machine.run_until_frame().unwrap();
    advance_until_satb_dma_finishes(&mut machine);
    assert_eq!(machine.devices().vdc().satb()[0], 0x1000);

    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0200);
    assert!(machine.devices().vdc().pending_satb_dma().is_some());
    machine.run_until_frame().unwrap();
    advance_until_satb_dma_finishes(&mut machine);
    assert_eq!(machine.devices().vdc().satb()[0], 0x2000);
    assert_eq!(machine.devices().vdc().satb()[255], 0x20FF);
}

#[test]
fn machine_satb_copy_is_visible_on_the_following_active_span() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0x0040);
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0010);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);

    let satb = machine.devices_mut().vdc_mut().vram_mut();
    satb[0x0100] = 64;
    satb[0x0101] = 32;
    satb[0x0102] = 0;
    satb[0x0103] = 0x008F;
    for plane in 0..4 {
        for row in 0..16 {
            satb[plane * 16 + row] = 0xFFFF;
        }
    }
    let vce = machine.devices_mut().vce_mut();
    vce.write_port(VcePort::from_offset(2), 0xFF);
    vce.write_port(VcePort::from_offset(3), 0x01);
    vce.write_port(VcePort::from_offset(4), 0x38);
    vce.write_port(VcePort::from_offset(5), 0x00);

    machine.run_until_frame().unwrap();
    machine.run_until_frame().unwrap();

    let first = usize::from(
        machine
            .presented_frame()
            .active_bounds()
            .unwrap()
            .first_row(),
    ) * PCE_ACTIVE_FRAME_WIDTH
        * 4;
    assert_eq!(
        &machine.framebuffer()[first..first + 4],
        &[0xFF, 0, 0, 0xFF]
    );
}

#[test]
fn machine_dma_service_contract_reports_in_progress_then_complete() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0100 + VDC_SATB_WORDS].fill(0x1234);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x0100);
    assert!(
        machine
            .devices_mut()
            .vdc_mut()
            .start_satb_dma_for_vertical_blank()
    );
    for remaining in (1..VDC_SATB_WORDS).rev() {
        assert_eq!(
            machine
                .devices_mut()
                .vdc_mut()
                .service_dma_slot(VdcDmaChannel::Satb)
                .unwrap(),
            VdcDmaProgress::Transferred {
                remaining_words: remaining as u32,
            }
        );
    }
    assert_eq!(
        machine
            .devices_mut()
            .vdc_mut()
            .service_dma_slot(VdcDmaChannel::Satb)
            .unwrap(),
        VdcDmaProgress::Complete
    );
}

#[test]
fn machine_satb_dma_mirrors_upper_sources_without_faulting() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    machine.devices_mut().vdc_mut().vram_mut()[0x7FFF] = 0xCAFE;
    machine.devices_mut().vdc_mut().vram_mut()[0] = 0xBEEF;
    write_vdc_register(machine.devices_mut(), VdcRegister::DmaControl, 0x0010);
    write_vdc_register(machine.devices_mut(), VdcRegister::SatbSource, 0x7FFF);

    machine.run_until_frame().unwrap();
    advance_until_satb_dma_finishes(&mut machine);
    assert_eq!(machine.devices().vdc().satb()[0], 0xCAFE);
    assert_eq!(machine.devices().vdc().satb()[1], 0xBEEF);
    assert!(!machine.faulted());
}

#[test]
fn sync_output_change_continues_machine_frames() {
    let mut machine = PceMachine::new(nonblack_video_rom()).unwrap();
    machine.run_until_frame().unwrap();
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0010);
    assert!(machine.devices().vdc().sync_output().horizontal());
    assert!(!machine.devices().vdc().sync_output().vertical());
    assert_eq!(machine.run_until_frame().unwrap().frames_published(), 1);
    assert!(!machine.faulted());
    let first = usize::from(
        machine
            .presented_frame()
            .active_bounds()
            .unwrap()
            .first_row(),
    ) * PCE_ACTIVE_FRAME_WIDTH
        * 4;
    assert_eq!(
        &machine.framebuffer()[first..first + 4],
        &[0xFF, 0, 0, 0xFF]
    );
}

#[test]
fn vdc_status_read_deasserts_machine_irq1_after_the_instruction() {
    let mut machine = PceMachine::new(rom_with_program(&[0xAD, 0x00, 0x00])).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0004);
    machine
        .devices_mut()
        .vdc_mut()
        .latch_status(VdcStatus::RASTER_MATCH);

    machine.step_boundary().unwrap();
    assert_eq!(
        machine.cpu().cpu().registers().a,
        VdcStatus::RASTER_MATCH.bits()
    );
    assert_eq!(machine.devices().vdc().status(), VdcStatus::empty());
    assert_eq!(
        machine.cpu().on_chip_io().read_irq(IrqPort::Request) & 0x02,
        0
    );
}

#[test]
fn checked_clock_arithmetic_reports_the_counter_and_operands() {
    for counter in [
        PceClockCounter::MasterTicks,
        PceClockCounter::VceLineAccumulator,
    ] {
        assert_eq!(
            checked_clock_add(u64::MAX - 1, 2, counter),
            Err(PceMachineError::ClockOverflow {
                counter,
                current: u64::MAX - 1,
                delta: 2,
            })
        );
    }
    assert_eq!(
        checked_clock_add(u64::MAX - 1, 1, PceClockCounter::MasterTicks),
        Ok(u64::MAX)
    );
}

#[test]
fn zero_length_block_transfer_advances_u64_master_and_vce_time() {
    let program = [0x73, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00];
    let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
    machine.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(machine.devices_mut().psg_mut());
    let mut reference = HuC6280Psg::new();
    reference.set_sample_rate(MAX_PSG_SAMPLE_RATE);
    configure_square_psg(&mut reference);
    configure_external_262(machine.devices_mut(), 0);
    set_red_backdrop(machine.devices_mut());
    machine
        .cpu_mut()
        .cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);

    let step = machine.step_boundary().unwrap();
    let expected_ticks = 393_233_u64 * PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE;
    let expected_lines = expected_ticks / PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE;
    assert!(matches!(
        step.action(),
        PceCpuAction::Instruction(cpu) if cpu.opcode == 0x73 && cpu.cycles == 393_233
    ));
    assert_eq!(step.master_ticks(), expected_ticks);
    assert_eq!(step.vce_lines(), expected_lines);
    assert_eq!(
        step.frames_published(),
        expected_lines / u64::from(VceFrameLength::Lines262.scanlines())
    );
    let total: u64 = machine.master_ticks();
    assert_eq!(total, expected_ticks);
    reference.advance_master_ticks(expected_ticks);
    let samples = drain_machine_audio(&mut machine);
    assert_eq!(samples, drain_psg_audio(&mut reference));
    assert!(
        (samples.len() / 2).abs_diff(expected_audio_frames(expected_ticks, MAX_PSG_SAMPLE_RATE))
            <= 1
    );
    let last_active_row = 260 * PCE_ACTIVE_FRAME_WIDTH * 4;
    assert_eq!(
        &machine.framebuffer()[last_active_row..last_active_row + 4],
        &[0xFF, 0, 0, 0xFF]
    );
}

#[test]
fn multiple_wraps_publish_complete_newest_frame_metadata() {
    let program = [0x73, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00];
    let mut machine = PceMachine::new(rom_with_program(&program)).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 63);
    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 1);
    machine
        .cpu_mut()
        .cpu_mut()
        .registers_mut()
        .status
        .insert(StatusFlags::INTERRUPT);

    let step = machine.step_boundary().unwrap();
    assert!(step.frames_published() > 1);
    let frame = machine.presented_frame();
    let bounds = frame.active_bounds().unwrap();
    assert_eq!(bounds.first_row(), 3);
    assert_eq!(bounds.row_end(), 261);
    assert_eq!(bounds.maximum_width(), 512);
    for line in [3, 260] {
        assert_eq!(frame.rows()[line].active_width(), 512);
        assert_eq!(
            frame.rows()[line].pixel_clock(),
            Some(VcePixelClock::DivideByThree)
        );
    }
}

#[test]
fn vce_control_selects_the_machine_frame_length_at_frame_start() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 0x04);

    let run = machine.run_until_frame().unwrap();
    let minimum_ticks =
        PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE * u64::from(VceFrameLength::Lines263.scanlines());
    assert_eq!(machine.vce_frame_length(), VceFrameLength::Lines263);
    assert_eq!(run.frames_published(), 1);
    assert!(run.master_ticks() >= minimum_ticks);
    assert!(run.master_ticks() < minimum_ticks + 12);
    assert_eq!(machine.vce_line_index(), 0);
}
