use super::{DEFAULT_SAMPLE_RATE, Emulator};
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cartridge::mappers::{FDS_BIOS_SIZE, FDS_HEADER_SIZE, FDS_SIDE_SIZE};
use crate::hardware::cartridge::{NesMapper, RomFormat};
use crate::hardware::constants::{APU_STATUS, FRAME_STEP_4, NMI_VECTOR_HI, NMI_VECTOR_LO, OAM_DMA};
use crate::hardware::cpu::StatusFlags;
use zeff_emu_common::debug::DebugEvent;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{ClockRate, MasterTicks};

fn build_test_rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg..prg + program.len()].copy_from_slice(program);
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn build_test_rom() -> Vec<u8> {
    build_test_rom_with_program(&[0xEA])
}

#[test]
fn timing_snapshot_tracks_and_restores_cpu_cycles() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).unwrap();
    let start = emu.timing_snapshot();
    assert_eq!(start.now(), MasterTicks::new(emu.cpu_cycles()));
    assert_eq!(start.rate(), ClockRate::from_ratio(21_477_272, 12));

    emu.step_instruction();
    let saved_clock = emu.timing_snapshot();
    let state = emu.encode_state().unwrap();
    emu.step_instruction();
    assert!(emu.timing_snapshot().now() > saved_clock.now());

    emu.load_state(&state).unwrap();
    assert_eq!(emu.timing_snapshot(), saved_clock);
}

#[test]
fn indexed_access_trace_uses_exact_cpu_ticks() {
    let rom = build_test_rom_with_program(&[0xA2, 0x01, 0xB5, 0x10]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).unwrap();
    emu.step_instruction();
    emu.bus.ram[0x11] = 0xA5;

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();
    let accesses: Vec<_> = events
        .iter()
        .map(|event| (event.at(), event.addr()))
        .collect();

    assert_eq!(cycles, 4);
    assert_eq!(
        accesses,
        vec![
            (Some(MasterTicks::new(start)), 0x8002),
            (Some(MasterTicks::new(start + 1)), 0x8003),
            (Some(MasterTicks::new(start + 2)), 0x0010),
            (Some(MasterTicks::new(start + 3)), 0x0011),
        ]
    );
}

#[test]
fn page_crossing_branch_traces_both_dummy_reads() {
    let mut program = vec![0xEA; 0x101];
    program[0xFD] = 0xD0;
    program[0xFE] = 0x01;
    let mut emu =
        Emulator::new(&build_test_rom_with_program(&program), DEFAULT_SAMPLE_RATE).unwrap();
    emu.cpu.pc = 0x80FD;

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();
    let accesses: Vec<_> = events
        .iter()
        .map(|event| (event.at(), event.addr()))
        .collect();

    assert_eq!(cycles, 4);
    assert_eq!(emu.cpu.pc, 0x8100);
    assert_eq!(
        accesses,
        vec![
            (Some(MasterTicks::new(start)), 0x80FD),
            (Some(MasterTicks::new(start + 1)), 0x80FE),
            (Some(MasterTicks::new(start + 2)), 0x80FF),
            (Some(MasterTicks::new(start + 3)), 0x8000),
        ]
    );
}

#[test]
fn jsr_trace_fetches_the_high_operand_after_stack_writes() {
    let mut emu = Emulator::new(
        &build_test_rom_with_program(&[0x20, 0x34, 0x12]),
        DEFAULT_SAMPLE_RATE,
    )
    .unwrap();

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();
    let accesses: Vec<_> = events
        .iter()
        .map(|event| (event.at(), event.addr()))
        .collect();

    assert_eq!(cycles, 6);
    assert_eq!(emu.cpu.pc, 0x1234);
    assert_eq!(
        accesses,
        vec![
            (Some(MasterTicks::new(start)), 0x8000),
            (Some(MasterTicks::new(start + 1)), 0x8001),
            (Some(MasterTicks::new(start + 2)), 0x01FD),
            (Some(MasterTicks::new(start + 3)), 0x01FD),
            (Some(MasterTicks::new(start + 4)), 0x01FC),
            (Some(MasterTicks::new(start + 5)), 0x8002),
        ]
    );
}

#[test]
fn nmi_trace_places_stack_and_vector_cycles() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).unwrap();
    emu.cpu.nmi_pending = true;

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();
    let accesses: Vec<_> = events
        .iter()
        .map(|event| (event.at(), event.addr()))
        .collect();

    assert_eq!(cycles, 7);
    assert_eq!(
        accesses,
        vec![
            (Some(MasterTicks::new(start)), 0x8000),
            (Some(MasterTicks::new(start + 1)), 0x8000),
            (Some(MasterTicks::new(start + 2)), 0x01FD),
            (Some(MasterTicks::new(start + 3)), 0x01FC),
            (Some(MasterTicks::new(start + 4)), 0x01FB),
            (Some(MasterTicks::new(start + 5)), u32::from(NMI_VECTOR_LO)),
            (Some(MasterTicks::new(start + 6)), u32::from(NMI_VECTOR_HI)),
        ]
    );
}

#[test]
fn oam_dma_source_reads_keep_their_stall_cycle_timestamps() {
    let rom = build_test_rom_with_program(&[0xA9, 0x02, 0x8D, 0x14, 0x40]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).unwrap();
    emu.step_instruction();

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();

    assert_eq!(cycles, 518);
    assert_eq!(events.len(), 260);
    assert_eq!(events[3].at(), Some(MasterTicks::new(start + 3)));
    assert_eq!(events[3].addr(), u32::from(OAM_DMA));
    assert_eq!(events[4].at(), Some(MasterTicks::new(start + 6)));
    assert_eq!(events[4].addr(), 0x0200);
    assert_eq!(events[259].at(), Some(MasterTicks::new(start + 516)));
    assert_eq!(events[259].addr(), 0x02FF);
}

#[test]
fn oam_dma_alignment_uses_the_write_cycle() {
    let rom = build_test_rom_with_program(&[0xA2, 0x00, 0xA9, 0x02, 0x9D, 0x14, 0x40]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).unwrap();
    emu.step_instruction();
    emu.step_instruction();

    let start = emu.cpu.cycles;
    let (_, _, cycles, events) = emu.step_instruction_with_bus_trace();

    assert_eq!(cycles, 518);
    assert_eq!(events[4].at(), Some(MasterTicks::new(start + 4)));
    assert_eq!(events[5].at(), Some(MasterTicks::new(start + 6)));
}

fn build_vs_system_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x8000 + 0x4000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 2;
    rom[6] = 0x30;
    rom[7] = 0x60;
    let prg = 16;
    rom[prg] = 0xEA;
    rom[prg + 0x7FFC] = 0x00;
    rom[prg + 0x7FFD] = 0x80;
    rom
}

fn build_fds_image(fill: u8) -> Vec<u8> {
    vec![fill; FDS_SIDE_SIZE]
}

#[test]
fn call_stack_tracks_jsr_and_rts() {
    let rom = build_test_rom_with_program(&[0x20, 0x06, 0x80, 0xEA, 0xEA, 0xEA, 0x60]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.set_opcode_log_enabled(true);

    emu.step_instruction();
    assert_eq!(emu.call_stack.len(), 1);
    assert_eq!(emu.call_stack[0].target, 0x8006);
    assert_eq!(emu.call_stack[0].return_address, 0x8003);
    assert_eq!(emu.call_stack[0].kind, crate::debug::CallStackKind::Call);

    emu.step_instruction();
    assert!(emu.call_stack.is_empty());
    let history = emu.recent_opcodes(2);
    assert_eq!(history[0].2, Some(6));
    assert_eq!(history[1].2, Some(0));
}

#[test]
fn guest_call_returns_to_suspended_context() {
    let rom = build_test_rom_with_program(&[0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xA9, 0x42, 0x60]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).unwrap();
    emu.debug_suspend();
    let pc = emu.cpu.pc;
    let sp = emu.cpu.sp;

    assert_eq!(emu.debug_execute_guest_call(0x8006, 10), Ok(2));
    assert_eq!(emu.cpu.regs.a, 0x42);
    assert_eq!(emu.cpu.pc, pc);
    assert_eq!(emu.cpu.sp, sp);
    assert_eq!(emu.cpu.state, crate::hardware::cpu::CpuState::Suspended);
}

#[test]
fn instruction_trace_captures_mapping_and_register_changes() {
    let rom = build_test_rom_with_program(&[0xA9, 0x42, 0xEA]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.set_instruction_trace_enabled(true);

    emu.step_instruction();
    emu.step_instruction();

    let entries: Vec<_> = emu.instruction_trace().iter().collect();
    assert_eq!(entries[0].pc, 0x8000);
    assert_eq!(entries[0].physical_rom_offset, Some(0));
    assert_eq!(entries[0].instruction_bytes(), &[0xA9, 0x42]);
    assert_eq!(entries[1].instruction_bytes(), &[0xEA]);
    assert!(
        entries[0]
            .register_deltas()
            .iter()
            .any(|delta| delta.register == 0)
    );
}

#[test]
fn event_breakpoints_stop_on_nmi_and_dma() {
    let rom = build_test_rom_with_program(&[0xA9, 0x02, 0x8D, 0x14, 0x40, 0xEA]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.set_event_breakpoint(DebugEvent::Interrupt, true);
    emu.cpu.nmi_pending = true;

    emu.step_instruction();
    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Interrupt));
    assert_eq!(emu.cpu.state, crate::hardware::cpu::CpuState::Suspended);

    emu.set_event_breakpoint(DebugEvent::Interrupt, false);
    emu.set_event_breakpoint(DebugEvent::Dma, true);
    emu.debug_continue();
    emu.cpu.pc = 0x8000;
    emu.step_instruction();
    emu.step_instruction();

    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Dma));
    assert_eq!(emu.cpu.state, crate::hardware::cpu::CpuState::Suspended);
}

#[test]
fn new_uses_power_on_reset_without_stack_adjust() {
    let emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    assert_eq!(emu.cpu.pc, 0x8000);
    assert_eq!(emu.cpu.sp, 0xFD);
    assert_eq!(emu.cpu.regs.a, 0);
    assert_eq!(emu.cpu.regs.x, 0);
    assert_eq!(emu.cpu.regs.y, 0);
    assert_eq!(emu.cpu.regs.p.bits(), 0x24);
}

#[test]
fn new_fds_builds_emulator_from_bios_and_disk_image() {
    let disk = build_fds_image(0x4A);
    let mut bios = vec![0xEA; FDS_BIOS_SIZE];
    bios[FDS_BIOS_SIZE - 4] = 0x00;
    bios[FDS_BIOS_SIZE - 3] = 0xE0;

    let emu = Emulator::new_fds(&disk, bios, DEFAULT_SAMPLE_RATE)
        .expect("FDS emulator should initialize");

    assert_eq!(emu.bus.cartridge.header().format, RomFormat::Nes2);
    assert_eq!(emu.bus.cartridge.header().mapper_kind(), NesMapper::Fds);
    assert_eq!(emu.cpu.pc, 0xE000);
    assert_eq!(emu.rom_crc32, crc32fast::hash(&disk));
    assert_eq!(emu.rom_hash, sha256(&disk));
}

#[test]
fn new_fds_canonical_hash_ignores_optional_fw_nes_header() {
    let raw = build_fds_image(0x9B);
    let mut headered = [0; FDS_HEADER_SIZE].to_vec();
    headered[..4].copy_from_slice(b"FDS\x1A");
    headered[4] = 1;
    headered.extend_from_slice(&raw);
    let bios = vec![0xEA; FDS_BIOS_SIZE];

    let raw_emu =
        Emulator::new_fds(&raw, bios.clone(), DEFAULT_SAMPLE_RATE).expect("raw FDS should load");
    let headered_emu =
        Emulator::new_fds(&headered, bios, DEFAULT_SAMPLE_RATE).expect("headered FDS should load");

    assert_eq!(raw_emu.rom_crc32, headered_emu.rom_crc32);
    assert_eq!(raw_emu.rom_hash, headered_emu.rom_hash);
}

#[test]
fn public_api_parity_wrappers_load_step_and_roundtrip_state() {
    let rom = build_test_rom_with_program(&[0x4C, 0x00, 0x80]);
    let mut emu = Emulator::from_rom_data(&rom).expect("test ROM");

    assert_eq!(emu.framebuffer_dimensions(), (256, 240));
    assert_eq!(emu.framebuffer().len(), 256 * 240 * 4);
    assert_eq!(emu.frame_count(), 0);
    assert_eq!(emu.save_ram_kind(), SaveRamKind::none());
    assert_eq!(emu.system_ram().len(), 0x800);
    assert_eq!(emu.video_ram_snapshot().len(), 0x2000);
    assert!(emu.iter_breakpoints().next().is_none());

    emu.add_breakpoint(emu.cpu_pc());
    assert_eq!(
        emu.iter_breakpoints().collect::<Vec<_>>(),
        vec![emu.cpu_pc()]
    );
    assert_eq!(emu.debug_hit_breakpoint(), None);
    emu.remove_breakpoint(emu.cpu_pc());

    emu.add_watchpoint(0x0000, crate::debug::WatchType::Write);
    assert_eq!(emu.debug_watchpoints().len(), 1);
    emu.cpu_write8(0x0000, 0x5A);
    assert_eq!(emu.cpu_peek8(0x0000), 0x5A);
    assert_eq!(
        emu.debug_hit_watchpoint().map(|hit| hit.new_value),
        Some(0x5A)
    );
    emu.debug_continue();

    emu.set_input(0x01, 0x01);
    emu.step_frame();

    assert!(emu.frame_count() > 0);

    let mut audio = Vec::new();
    emu.drain_audio_samples_into(&mut audio);

    let state = emu
        .encode_state()
        .expect("NES emulator should encode state");
    emu.load_state(&state)
        .expect("NES emulator should load state");
}

#[test]
fn public_cpu_peek_does_not_mutate_controller_or_trace() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input(0x01, 0);
    emu.cpu_write(0x4016, 1);
    emu.cpu_write(0x4016, 0);
    emu.bus.debug_trace_enabled = true;
    assert_eq!(emu.cpu_peek8(0x4016), 0);

    assert!(emu.bus.debug_trace_events.is_empty());
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x01, 1);
}

#[test]
fn public_cpu_peek_applies_game_genie_without_trace() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.add_game_genie_patch(crate::cheats::NesGameGeniePatch {
        address: 0x8000,
        value: 0x42,
        compare: Some(0xEA),
    });
    emu.bus.debug_trace_enabled = true;

    assert_eq!(emu.cpu_peek8(0x8000), 0x42);
    assert!(emu.bus.debug_trace_events.is_empty());

    emu.clear_game_genie();
    assert_eq!(emu.cpu_peek8(0x8000), 0xEA);
}

#[test]
fn host_input_mapping_is_owned_by_nes_core_for_both_ports() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input(0x05, 0x05);
    assert_eq!(
        read_standard_controller_bits(&mut emu, 0x4016),
        [1, 0, 1, 0, 1, 0, 0, 1]
    );

    emu.set_input_p2(0x0A, 0x0A);
    assert_eq!(
        read_standard_controller_bits(&mut emu, 0x4017),
        [0, 1, 0, 1, 0, 1, 1, 0]
    );
}

#[test]
fn reset_preserves_cpu_registers_and_decrements_stack() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.cpu.regs.a = 0x34;
    emu.cpu.regs.x = 0x56;
    emu.cpu.regs.y = 0x78;
    emu.cpu.regs.p = StatusFlags::from_bits_truncate(0xFB);
    emu.cpu.sp = 0x12;
    emu.bus.ram[0x110] = 0xBC;
    emu.bus.ram[0x111] = 0x9A;
    emu.bus.ram[0x112] = 0xFB;

    emu.reset();

    assert_eq!(emu.cpu.pc, 0x8000);
    assert_eq!(emu.cpu.regs.a, 0x34);
    assert_eq!(emu.cpu.regs.x, 0x56);
    assert_eq!(emu.cpu.regs.y, 0x78);
    assert_eq!(emu.cpu.regs.p.bits(), 0xFF);
    assert_eq!(emu.cpu.sp, 0x0F);
    assert_eq!(emu.bus.ram[0x110], 0xBC);
    assert_eq!(emu.bus.ram[0x111], 0x9A);
    assert_eq!(emu.bus.ram[0x112], 0xFB);
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    Sha256::digest(bytes).into()
}

#[test]
fn mapper_99_zapper_uses_vs_serial_protocol_on_4016() {
    let mut emu =
        Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.set_zapper_state(true, true, true, None);

    emu.cpu_write(0x4016, 1);
    emu.cpu_write(0x4016, 0);

    let port_1_bits: Vec<u8> = (0..8)
        .map(|_| emu.bus_mut().cpu_read(0x4016) & 0x01)
        .collect();

    assert_eq!(port_1_bits, [0, 0, 0, 0, 1, 0, 1, 1]);
}

#[test]
fn mapper_99_select_exposes_one_vs_coin_pulse() {
    let mut emu =
        Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input(0x04, 0);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0x20);

    emu.set_input(0, 0);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x04, 0);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0x20);

    for _ in 0..4 {
        emu.bus.finish_vs_system_input_frame();
    }
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0);
}

#[test]
fn nrom_select_does_not_expose_vs_credit_bits() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input(0x04, 0);

    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0);
}

#[test]
fn indexed_store_dummy_read_can_ack_frame_irq_edge() {
    let rom = build_test_rom_with_program(&[
        0xA2, 0x15, // LDX #$15
        0xA9, 0x00, // LDA #$00
        0x9D, 0x00, 0x40, // STA $4000,X
        0xEA, // NOP
    ]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.step_instruction();
    emu.step_instruction();

    emu.bus.apu.five_step_mode = false;
    emu.bus.apu.irq_inhibit = false;
    emu.bus.apu.frame_irq = false;
    emu.bus.apu.frame_cycle = FRAME_STEP_4 - 3;
    emu.bus.apu.frame_reset_delay = 0;

    let (_, _, _, events) = emu.step_instruction_with_bus_trace();

    let status_read = events.iter().find_map(|event| match event {
        DebugTraceEvent::Read { addr, value, .. } if *addr == u32::from(APU_STATUS) => Some(*value),
        _ => None,
    });

    assert_eq!(status_read.map(|value| value & 0x40), Some(0x40));
    assert!(!emu.bus.apu.irq_pending());
    assert!(!emu.cpu.irq_line);
}

fn read_standard_controller_bits(emu: &mut Emulator, port: u16) -> [u8; 8] {
    emu.cpu_write(0x4016, 1);
    emu.cpu_write(0x4016, 0);
    std::array::from_fn(|_| emu.bus_mut().cpu_read(port) & 0x01)
}
