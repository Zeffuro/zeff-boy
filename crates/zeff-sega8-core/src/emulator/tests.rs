use super::{
    Emulator, HOST_BUTTON_1, HOST_BUTTON_2, HOST_BUTTON_START, HOST_DPAD_LEFT, HOST_DPAD_RIGHT,
    HOST_DPAD_UP, SMS_PAD_BUTTON_1, SMS_PAD_BUTTON_2, SMS_PAD_DOWN, SMS_PAD_LEFT, SMS_PAD_RIGHT,
    SMS_PAD_UP, Sega8LoadConfig,
};
use crate::hardware::bus::CpuAccessTraceEvent;
use crate::hardware::cartridge::{HeaderLocation, Sega8MapperKind, Sega8System, SystemHint};
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, IO_PORT_CONTROL, IO_PORT_CONTROLLER_2, IO_PORT_GG_SERIAL_CONTROL,
    IO_PORT_GG_SERIAL_RX, IO_PORT_GG_SERIAL_TX, IO_PORT_GG_START, IO_PORT_VDP_CONTROL,
    IO_PORT_VDP_DATA, RGBA_CHANNELS, SEGA_HEADER_MAGIC, SEGA_HEADER_SIZE, SMS_MODE4_TILE_BYTES,
    SMS_SCREEN_H, SMS_SCREEN_W, VDP_CONTROL_REGISTER_WRITE_VALUE, VDP_REG0_MODE4,
    VDP_REG1_FRAME_IRQ_ENABLE, VDP_REGISTER_MODE_CONTROL_2, VDP_STATUS_VBLANK,
};
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_emu_common::debug::{DebugEvent, WatchType};
use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{ClockRate, MasterTicks};

fn rom_with_header(location: HeaderLocation, region_size: u8) -> Vec<u8> {
    let mut rom = vec![0xFF; location.offset() + SEGA_HEADER_SIZE];
    let offset = location.offset();
    rom[offset..offset + SEGA_HEADER_MAGIC.len()].copy_from_slice(SEGA_HEADER_MAGIC);
    rom[offset + 0x0F] = region_size;
    rom
}

#[test]
fn timing_snapshot_tracks_video_standard_and_state() {
    let mut rom = rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C);
    rom[0] = 0x00;
    let mut emu = Emulator::new_with_hint_and_video_standard(
        &rom,
        48_000,
        SystemHint::MasterSystem,
        Sega8VideoStandard::Pal,
    )
    .unwrap();
    let rate = u64::from(Sega8VideoStandard::Pal.clock_hz_approx());
    assert_eq!(emu.timing_snapshot().rate(), ClockRate::from_hz(rate));
    assert_eq!(emu.timing_snapshot().now(), MasterTicks::new(0));

    emu.step_instruction();
    let saved_clock = emu.timing_snapshot();
    let state = emu.encode_state().unwrap();
    emu.step_instruction();
    assert!(emu.timing_snapshot().now() > saved_clock.now());

    emu.load_state(&state).unwrap();
    assert_eq!(emu.timing_snapshot(), saved_clock);
}

fn set_vdp_write_address(emu: &mut Emulator, addr: u16) {
    emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, addr as u8);
    emu.bus_mut()
        .io_write(IO_PORT_VDP_CONTROL, 0x40 | ((addr >> 8) as u8 & 0x3F));
}

fn set_vdp_cram_write_address(emu: &mut Emulator, addr: u8) {
    emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, addr);
    emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, 0xC0);
}

fn set_vdp_register(emu: &mut Emulator, register: u8, value: u8) {
    emu.bus_mut().io_write(IO_PORT_VDP_CONTROL, value);
    emu.bus_mut().io_write(
        IO_PORT_VDP_CONTROL,
        VDP_CONTROL_REGISTER_WRITE_VALUE | register,
    );
}

#[test]
fn creates_master_system_emulator_from_auto_header() {
    let emu = Emulator::new(&rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C), 44_100)
        .expect("SMS emulator should initialize");

    assert_eq!(emu.system(), Sega8System::MasterSystem);
    assert_eq!(emu.framebuffer_dimensions(), (SMS_SCREEN_W, SMS_SCREEN_H));
    assert_eq!(
        emu.framebuffer().len(),
        SMS_SCREEN_W * SMS_SCREEN_H * RGBA_CHANNELS
    );
    assert_eq!(emu.sample_rate(), 44_100);
}

#[test]
fn instruction_trace_captures_z80_bytes_and_mapping() {
    let mut rom = rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C);
    rom[..2].copy_from_slice(&[0x3E, 0x42]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.set_instruction_trace_enabled(true);

    emu.step_instruction();

    let entry = emu.instruction_trace().iter().next().unwrap();
    assert_eq!(entry.pc, 0);
    assert_eq!(entry.physical_rom_offset, Some(0));
    assert_eq!(&entry.instruction[..2], &[0x3E, 0x42]);
}

#[test]
fn guest_call_returns_to_suspended_context() {
    let mut rom = rom_with_header(HeaderLocation::Offset0x7ff0, 0x4C);
    rom[0x100..0x103].copy_from_slice(&[0x3E, 0x42, 0xC9]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.debug_suspend();
    let regs = emu.cpu().regs();

    assert_eq!(emu.debug_execute_guest_call(0x0100, 10), Ok(2));
    assert_eq!(emu.cpu().regs().a, 0x42);
    assert_eq!(emu.cpu().regs().pc, regs.pc);
    assert_eq!(emu.cpu().regs().sp, regs.sp);
    assert!(emu.is_suspended());
}

#[test]
fn creates_game_gear_emulator_from_auto_header() {
    let emu = Emulator::new(&rom_with_header(HeaderLocation::Offset0x3ff0, 0x7A), 48_000)
        .expect("GG emulator should initialize");

    assert_eq!(emu.system(), Sega8System::GameGear);
    assert_eq!(emu.framebuffer_dimensions(), (GG_SCREEN_W, GG_SCREEN_H));
    assert_eq!(
        emu.framebuffer().len(),
        GG_SCREEN_W * GG_SCREEN_H * RGBA_CHANNELS
    );
}

#[test]
fn path_hint_selects_sg1000_for_headerless_sg_rom() {
    let emu = Emulator::new_with_path_hint(&[0x00, 0x76], 48_000, std::path::Path::new("a.sg"))
        .expect("SG emulator should initialize from path hint");

    assert_eq!(emu.system(), Sega8System::Sg1000);
    assert_eq!(emu.framebuffer_dimensions(), (SMS_SCREEN_W, SMS_SCREEN_H));
}

#[test]
fn path_hint_selects_pal_video_standard_from_region_tag() {
    let emu = Emulator::new_with_path_hint(
        &[0x00, 0x76],
        48_000,
        std::path::Path::new("Example (Europe).sms"),
    )
    .expect("SMS emulator should initialize from path hint");

    assert_eq!(emu.video_standard(), Sega8VideoStandard::Pal);
    assert_eq!(emu.bus().vdp().total_scanlines(), 313);
}

#[test]
fn interrupt_event_breakpoint_suspends_emulator() {
    let mut emu =
        Emulator::new_with_hint(&[0xFB, 0x00, 0x00], 48_000, SystemHint::MasterSystem).unwrap();
    emu.set_event_breakpoint(DebugEvent::Interrupt, true);
    emu.step_instruction();
    emu.step_instruction();
    set_vdp_register(
        &mut emu,
        VDP_REGISTER_MODE_CONTROL_2 as u8,
        VDP_REG1_FRAME_IRQ_ENABLE,
    );
    emu.bus_mut().vdp_mut().set_status_bits(VDP_STATUS_VBLANK);

    emu.step_instruction();

    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Interrupt));
    assert!(emu.is_suspended());
}

#[test]
fn path_hint_selects_japanese_console_region_from_region_tag() {
    let emu = Emulator::new_with_path_hint(
        &[0x00, 0x76],
        48_000,
        std::path::Path::new("Example (Japan).gg"),
    )
    .expect("GG emulator should initialize from path hint");

    assert_eq!(emu.console_region(), Sega8Region::Japanese);
    assert_eq!(emu.bus().console_region(), Sega8Region::Japanese);
}

#[test]
fn header_region_can_select_japanese_console_region() {
    let emu = Emulator::new_with_hint(
        &rom_with_header(HeaderLocation::Offset0x7ff0, 0x30),
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("SMS emulator should initialize from header");

    assert_eq!(emu.console_region(), Sega8Region::Japanese);
}

#[test]
fn header_console_region_takes_precedence_over_path_tag() {
    let emu = Emulator::new_with_path_hint(
        &rom_with_header(HeaderLocation::Offset0x7ff0, 0x30),
        48_000,
        std::path::Path::new("Example [E].sms"),
    )
    .expect("SMS emulator should initialize from header and path hint");

    assert_eq!(emu.video_standard(), Sega8VideoStandard::Pal);
    assert_eq!(emu.console_region(), Sega8Region::Japanese);
}

#[test]
fn load_config_matches_explicit_system_hint_constructors() {
    let rom = [0x00, 0x76];
    for (hint, expected_system, expected_dimensions) in [
        (
            SystemHint::MasterSystem,
            Sega8System::MasterSystem,
            (SMS_SCREEN_W, SMS_SCREEN_H),
        ),
        (
            SystemHint::GameGear,
            Sega8System::GameGear,
            (GG_SCREEN_W, GG_SCREEN_H),
        ),
        (
            SystemHint::Sg1000,
            Sega8System::Sg1000,
            (SMS_SCREEN_W, SMS_SCREEN_H),
        ),
    ] {
        let legacy = Emulator::new_with_hint(&rom, 44_100, hint)
            .expect("legacy constructor should initialize");
        let configured =
            Emulator::new_with_config(&rom, Sega8LoadConfig::new(44_100).with_system_hint(hint))
                .expect("config constructor should initialize");

        assert_eq!(configured.system(), expected_system);
        assert_eq!(configured.system(), legacy.system());
        assert_eq!(configured.framebuffer_dimensions(), expected_dimensions);
        assert_eq!(
            configured.framebuffer_dimensions(),
            legacy.framebuffer_dimensions()
        );
        assert_eq!(configured.sample_rate(), legacy.sample_rate());
    }
}

#[test]
fn load_config_path_helper_matches_path_hint_constructor() {
    let path = std::path::Path::new("Example (Europe).gg");
    let legacy = Emulator::new_with_path_hint(&[0x00, 0x76], 48_000, path)
        .expect("legacy path constructor should initialize");
    let configured =
        Emulator::new_with_config(&[0x00, 0x76], Sega8LoadConfig::from_path(48_000, path))
            .expect("config path helper should initialize");

    assert_eq!(configured.system(), legacy.system());
    assert_eq!(configured.video_standard(), Sega8VideoStandard::Pal);
    assert_eq!(configured.video_standard(), legacy.video_standard());
    assert_eq!(configured.console_region(), Sega8Region::Export);
    assert_eq!(configured.console_region(), legacy.console_region());
}

#[test]
fn load_config_path_helper_applies_explicit_mapper_tag() {
    let path = std::path::Path::new("Example [mapper=janggun].sms");
    let configured =
        Emulator::new_with_config(&[0x00, 0x76], Sega8LoadConfig::from_path(48_000, path))
            .expect("config path helper should initialize");

    assert_eq!(configured.system(), Sega8System::MasterSystem);
    assert_eq!(configured.bus().mapper().kind(), Sega8MapperKind::Janggun);
}

#[test]
fn load_config_matches_video_region_and_sample_rate_constructor() {
    let rom = rom_with_header(HeaderLocation::Offset0x7ff0, 0x30);
    let legacy = Emulator::new_with_hint_video_standard_region_fallback(
        &rom,
        0,
        SystemHint::MasterSystem,
        Sega8VideoStandard::Pal,
        Some(Sega8Region::Export),
        Some(Sega8Region::Japanese),
    )
    .expect("legacy full constructor should initialize");
    let configured = Emulator::new_with_config(
        &rom,
        Sega8LoadConfig::new(0)
            .with_system_hint(SystemHint::MasterSystem)
            .with_video_standard(Sega8VideoStandard::Pal)
            .with_console_region(Some(Sega8Region::Export))
            .with_console_region_fallback(Some(Sega8Region::Japanese)),
    )
    .expect("config full constructor should initialize");

    assert_eq!(configured.system(), legacy.system());
    assert_eq!(configured.video_standard(), Sega8VideoStandard::Pal);
    assert_eq!(configured.video_standard(), legacy.video_standard());
    assert_eq!(configured.console_region(), Sega8Region::Export);
    assert_eq!(configured.console_region(), legacy.console_region());
    assert_eq!(configured.sample_rate(), super::DEFAULT_SAMPLE_RATE);
    assert_eq!(configured.sample_rate(), legacy.sample_rate());
}

#[test]
fn load_config_can_force_mapper_kind_without_changing_system_hint() {
    let rom = vec![0; crate::hardware::constants::ROM_BANK_SIZE * 4];
    let emu = Emulator::new_with_config(
        &rom,
        Sega8LoadConfig::new(48_000)
            .with_system_hint(SystemHint::MasterSystem)
            .with_mapper_kind(Some(Sega8MapperKind::Korean)),
    )
    .expect("config should initialize with forced mapper");

    assert_eq!(emu.system(), Sega8System::MasterSystem);
    assert_eq!(emu.bus().cartridge.mapper_kind(), Sega8MapperKind::Korean);
    assert_eq!(emu.bus().mapper().kind(), Sega8MapperKind::Korean);
}

#[test]
fn step_frame_renders_sg1000_tms9918_background_from_vdp_state() {
    let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::Sg1000)
        .expect("SG emulator should initialize");

    assert_eq!(emu.frame_count(), 0);
    assert!(emu.framebuffer().iter().all(|&byte| byte == 0));

    set_vdp_register(&mut emu, 1, 0x40);
    set_vdp_register(&mut emu, 2, 0x0E);
    set_vdp_register(&mut emu, 3, 0x20);
    set_vdp_register(&mut emu, 4, 0x00);
    set_vdp_register(&mut emu, 7, 0x01);
    set_vdp_write_address(&mut emu, 8);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x80);
    set_vdp_write_address(&mut emu, 0x0800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x60);
    set_vdp_write_address(&mut emu, 0x3800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);

    emu.step_frame();

    assert_eq!(emu.frame_count(), 1);
    assert_eq!(
        &emu.framebuffer()[..RGBA_CHANNELS],
        &[0xD4, 0x52, 0x4D, 0xFF]
    );
}

#[test]
fn step_frame_renders_sms_tms9918_background_when_mode4_is_disabled() {
    let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem)
        .expect("SMS emulator should initialize");

    set_vdp_register(&mut emu, 1, 0x40);
    set_vdp_register(&mut emu, 2, 0x0E);
    set_vdp_register(&mut emu, 3, 0x20);
    set_vdp_register(&mut emu, 4, 0x00);
    set_vdp_register(&mut emu, 7, 0x01);
    set_vdp_write_address(&mut emu, 8);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x80);
    set_vdp_write_address(&mut emu, 0x0800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x60);
    set_vdp_write_address(&mut emu, 0x3800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);

    emu.step_frame();

    assert_eq!(
        &emu.framebuffer()[..RGBA_CHANNELS],
        &[0xD4, 0x52, 0x4D, 0xFF]
    );
}

#[test]
fn step_frame_renders_game_gear_tms9918_background_from_cropped_cram_palette() {
    let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::GameGear)
        .expect("GG emulator should initialize");

    set_vdp_register(&mut emu, 1, 0x40);
    set_vdp_register(&mut emu, 2, 0x0E);
    set_vdp_register(&mut emu, 3, 0x20);
    set_vdp_register(&mut emu, 4, 0x00);
    set_vdp_register(&mut emu, 7, 0x01);
    set_vdp_write_address(&mut emu, 8);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x80);
    set_vdp_write_address(&mut emu, 0x0800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x60);
    set_vdp_write_address(&mut emu, 0x3800 + 3 * 32 + 6);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);
    set_vdp_cram_write_address(&mut emu, 44);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x0F);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x00);

    emu.step_frame();

    assert_eq!(
        &emu.framebuffer()[..RGBA_CHANNELS],
        &[0xFF, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn step_frame_renders_sms_mode4_background_from_vdp_state() {
    let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
        .expect("SMS emulator should initialize");

    set_vdp_register(&mut emu, 0, VDP_REG0_MODE4);
    set_vdp_register(&mut emu, 2, 0x0E);
    set_vdp_register(&mut emu, 1, 0x40);
    set_vdp_write_address(&mut emu, SMS_MODE4_TILE_BYTES as u16);
    for value in [0x80, 0x80, 0x00, 0x00] {
        emu.bus_mut().io_write(IO_PORT_VDP_DATA, value);
    }
    set_vdp_write_address(&mut emu, 0x3800);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 1);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0);
    set_vdp_cram_write_address(&mut emu, 3);
    emu.bus_mut().io_write(IO_PORT_VDP_DATA, 0x03);

    emu.step_frame();

    assert_eq!(
        &emu.framebuffer()[0..RGBA_CHANNELS],
        &[0xFF, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn set_input_maps_host_masks_to_active_low_sms_controller_bits() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.set_input(HOST_BUTTON_1, HOST_DPAD_RIGHT | HOST_DPAD_UP);

    let raw = emu
        .bus()
        .input()
        .read_controller(crate::hardware::input::ControllerPort::One);
    assert_eq!(raw & SMS_PAD_BUTTON_1, 0);
    assert_eq!(raw & SMS_PAD_RIGHT, 0);
    assert_eq!(raw & SMS_PAD_UP, 0);
    assert_ne!(raw & SMS_PAD_BUTTON_2, 0);
    assert_ne!(raw & SMS_PAD_LEFT, 0);
    assert_ne!(raw & SMS_PAD_DOWN, 0);
}

#[test]
fn set_input_maps_start_to_game_gear_start_port_only() {
    let mut gg = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::GameGear)
        .expect("GG emulator should initialize");
    gg.set_input(HOST_BUTTON_START, 0);

    assert!(gg.bus().input().game_gear_start_pressed());
    assert_eq!(gg.bus_mut().io_read(IO_PORT_GG_START), 0x7F);

    let mut sms = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("SMS emulator should initialize");
    sms.set_input(HOST_BUTTON_START, 0);

    assert!(sms.bus().input().game_gear_start_pressed());
    assert_eq!(sms.bus_mut().io_read(IO_PORT_GG_START), 0xFF);
}

#[test]
fn set_input_p2_maps_host_masks_to_second_controller_bits() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.set_input_p2(
        HOST_BUTTON_2 | HOST_BUTTON_START,
        HOST_DPAD_LEFT | HOST_DPAD_UP,
    );

    let raw = emu
        .bus()
        .input()
        .read_controller(crate::hardware::input::ControllerPort::Two);
    assert_eq!(raw & SMS_PAD_BUTTON_2, 0);
    assert_eq!(raw & SMS_PAD_LEFT, 0);
    assert_eq!(raw & SMS_PAD_UP, 0);
    assert_ne!(raw & SMS_PAD_BUTTON_1, 0);
    assert_ne!(raw & SMS_PAD_RIGHT, 0);
    assert_ne!(raw & SMS_PAD_DOWN, 0);
    assert!(
        !emu.bus().input().game_gear_start_pressed(),
        "host Start is a Game Gear system button and must not affect P2"
    );
}

#[test]
fn game_gear_link_peer_sync_transfers_serial_bytes_between_emulators() {
    let mut left = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::GameGear)
        .expect("left GG emulator should initialize");
    let mut right = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::GameGear)
        .expect("right GG emulator should initialize");

    left.bus_mut().io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
    right.bus_mut().io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
    left.bus_mut().io_write(IO_PORT_GG_SERIAL_TX, 0x5A);

    left.sync_game_gear_link_peer(&mut right);

    assert_ne!(left.bus_mut().io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x01, 0);
    assert_eq!(right.bus_mut().io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);

    left.bus_mut().step_cycles(20_000);
    right.bus_mut().step_cycles(20_000);
    left.sync_game_gear_link_peer(&mut right);

    assert_eq!(left.bus_mut().io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x05, 0);
    assert_ne!(right.bus_mut().io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    assert_eq!(right.bus_mut().io_read(IO_PORT_GG_SERIAL_RX), 0x5A);
}

#[test]
fn sms_region_detector_observes_export_th_loopback() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("SMS emulator should initialize");

    emu.bus_mut().io_write(IO_PORT_CONTROL, 0xF5);
    assert_eq!(emu.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);

    emu.bus_mut().io_write(IO_PORT_CONTROL, 0x55);
    assert_eq!(emu.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);
}

#[test]
fn sms_region_detector_observes_japanese_pbc_inverted_th_loopback() {
    let mut emu = Emulator::new_with_hint_video_standard_and_region(
        &[0x00],
        48_000,
        SystemHint::MasterSystem,
        Sega8VideoStandard::Ntsc,
        Some(Sega8Region::JapanesePowerBaseConverter),
    )
    .expect("SMS emulator should initialize");

    emu.bus_mut().io_write(IO_PORT_CONTROL, 0xF5);
    assert_eq!(emu.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);

    emu.bus_mut().io_write(IO_PORT_CONTROL, 0x55);
    assert_eq!(emu.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);
}

#[test]
fn suspended_emulator_does_not_advance() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.suspend();
    emu.step_frame();

    assert_eq!(emu.frame_count(), 0);
    assert!(emu.is_suspended());
}

#[test]
fn standard_mapper_ram_is_not_blindly_exposed_as_battery_sram() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    assert_eq!(
        emu.save_ram_kind(),
        SaveRamKind::mapper_ram_unknown(crate::hardware::constants::SMS_CARTRIDGE_RAM_SIZE)
    );
    assert!(!emu.has_battery());
    assert_eq!(emu.dump_battery_sram(), None);
    assert!(emu.load_battery_sram(&[0; 1]).is_err());
    assert_eq!(
        emu.system_ram().len(),
        crate::hardware::constants::SMS_WORK_RAM_SIZE
    );
    assert_eq!(
        emu.video_ram_snapshot().len(),
        crate::hardware::constants::SMS_VRAM_SIZE
    );
    assert_eq!(
        emu.palette_ram_snapshot().len(),
        crate::hardware::constants::SMS_CRAM_SIZE
    );
    assert!(!emu.is_cpu_suspended());
    emu.add_watchpoint(0xC000, WatchType::Write);
    emu.cpu_write8(0xC000, 0x5A);
    assert_eq!(emu.cpu_peek8(0xC000), 0x5A);
    assert_eq!(
        emu.debug_hit_watchpoint().map(|hit| hit.new_value),
        Some(0x5A)
    );
    emu.debug_continue();

    emu.bus_mut().cpu_write(
        crate::hardware::constants::MAPPER_FRAME_CONTROL,
        crate::hardware::constants::MAPPER_FRAME_CONTROL_CART_RAM_ENABLE,
    );
    emu.bus_mut().cpu_write(0x8000, 0x5A);

    assert_eq!(emu.bus().cartridge_ram()[0], 0x5A);
    assert_eq!(emu.dump_battery_sram(), None);
}

#[test]
fn public_rom_patch_api_matches_cpu_read_path() {
    let mut rom = vec![0; crate::hardware::constants::ROM_BANK_SIZE * 2];
    rom[0x1234] = 0x56;
    let mut emu = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.add_rom_patch(CheatPatch::RomWriteIfEquals {
        address: 0x1234,
        value: CheatValue::Constant(0x9A),
        compare: CheatValue::Constant(0x56),
    });

    assert_eq!(emu.rom_patches().len(), 1);
    assert_eq!(emu.cpu_peek8(0x1234), 0x9A);

    emu.clear_rom_patches();
    assert!(emu.rom_patches().is_empty());
    assert_eq!(emu.cpu_peek8(0x1234), 0x56);
}

#[test]
fn breakpoint_suspends_before_instruction_executes() {
    let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.add_breakpoint(0);
    let fetched = emu.step_instruction();

    assert_eq!(fetched, None);
    assert!(emu.is_suspended());
    assert_eq!(emu.cpu().cycles(), 0);
    assert_eq!(emu.cpu().regs().pc, 0);
    assert_eq!(emu.debug_hit_breakpoint(), Some(0));
}

#[test]
fn debug_step_executes_one_instruction_while_suspended_on_breakpoint() {
    let mut emu = Emulator::new_with_hint(&[0x00, 0x76], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.add_breakpoint(0);
    assert_eq!(emu.step_instruction(), None);
    emu.debug_step();

    assert!(emu.is_suspended());
    assert_eq!(emu.debug_hit_breakpoint(), None);
    assert_eq!(emu.cpu().regs().pc, 1);
    assert_eq!(emu.cpu().cycles(), 4);
}

#[test]
fn write_watchpoint_suspends_after_cpu_write() {
    let mut emu = Emulator::new_with_hint(
        &[0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76],
        48_000,
        SystemHint::Sg1000,
    )
    .expect("emulator should initialize");

    emu.add_watchpoint(0xC000, WatchType::Write);
    emu.step_instruction();
    assert!(!emu.is_suspended());
    assert!(emu.debug_hit_watchpoint().is_none());

    emu.step_instruction();
    let hit = emu
        .debug_hit_watchpoint()
        .expect("write watchpoint should hit");

    assert!(emu.is_suspended());
    assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
    assert_eq!(hit.address, 0xC000);
    assert_eq!(hit.old_value, 0);
    assert_eq!(hit.new_value, 0x5A);
    assert_eq!(hit.watch_type, WatchType::Write);
}

#[test]
fn read_watchpoint_suspends_after_cpu_read() {
    let mut emu =
        Emulator::new_with_hint(&[0x3A, 0x00, 0xC0, 0x76], 48_000, SystemHint::MasterSystem)
            .expect("emulator should initialize");
    emu.bus_mut().cpu_write(0xC000, 0xA5);

    emu.add_watchpoint(0xC000, WatchType::Read);
    emu.step_instruction();
    let hit = emu
        .debug_hit_watchpoint()
        .expect("read watchpoint should hit");

    assert!(emu.is_suspended());
    assert_eq!(emu.cpu().regs().a, 0xA5);
    assert_eq!(hit.address, 0xC000);
    assert_eq!(hit.old_value, 0xA5);
    assert_eq!(hit.new_value, 0xA5);
    assert_eq!(hit.watch_type, WatchType::Read);
}

#[test]
fn debug_write_triggers_write_watchpoint() {
    let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.add_watchpoint(0xC000, WatchType::Write);
    emu.debug_write(0xC000, 0x5A);

    let hit = emu
        .debug_hit_watchpoint()
        .expect("debug write should hit watchpoint");
    assert!(emu.is_suspended());
    assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
    assert_eq!(hit.address, 0xC000);
    assert_eq!(hit.old_value, 0);
    assert_eq!(hit.new_value, 0x5A);
}

#[test]
fn bus_trace_preserves_mixed_memory_access_order_without_timestamps() {
    let mut emu = Emulator::new_with_hint(
        &[0x21, 0x00, 0xC0, 0x34, 0x76],
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("emulator should initialize");

    emu.step_instruction();
    let (_, trace) = emu.step_instruction_with_bus_trace();
    assert!(matches!(
        trace.as_slice(),
        [
            CpuAccessTraceEvent::Read {
                at: None,
                space: TraceWriteKind::Memory,
                addr: 0x0003,
                value: 0x34,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            },
            CpuAccessTraceEvent::Read {
                at: None,
                space: TraceWriteKind::Memory,
                addr: 0xC000,
                value: 0,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            },
            CpuAccessTraceEvent::Write {
                at: None,
                space: TraceWriteKind::Memory,
                addr: 0xC000,
                old_value: 0,
                written_value: 1,
                new_value: 1,
                width: TraceWriteWidth::Byte,
                mapped_addr: None,
            },
        ]
    ));
}

#[test]
fn public_cpu_peek_does_not_enter_bus_trace() {
    let mut emu = Emulator::new_with_hint(&[0x3E, 0x5A, 0x76], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.bus_mut().begin_cpu_access_trace();
    assert_eq!(emu.cpu_peek8(0x0000), 0x3E);

    assert!(emu.bus_mut().drain_cpu_access_trace().is_empty());
}

#[test]
fn bus_trace_records_io_writes_for_out_instruction() {
    let mut emu = Emulator::new_with_hint(
        &[0x3E, 0x90, 0xD3, 0x7F, 0x76],
        48_000,
        SystemHint::MasterSystem,
    )
    .expect("emulator should initialize");

    emu.step_instruction();
    let (fetched, trace) = emu.step_instruction_with_bus_trace();

    assert_eq!(
        fetched.expect("OUT instruction should execute").opcode,
        0xD3
    );
    assert!(trace.iter().any(|event| matches!(
        event,
        CpuAccessTraceEvent::Write {
            at: None,
            space: TraceWriteKind::Io,
            addr: 0x7F,
            old_value: 0x90,
            written_value: 0x90,
            new_value: 0x90,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    )));
}

#[test]
fn step_instruction_runs_minimal_z80_program() {
    let mut emu = Emulator::new_with_hint(
        &[0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76],
        48_000,
        SystemHint::Sg1000,
    )
    .expect("emulator should initialize");

    while !emu.cpu().is_halted() {
        emu.step_instruction();
    }

    assert_eq!(emu.bus().cpu_read(0xC000), 0x5A);
    assert_eq!(emu.cpu().cycles(), 24);
}

#[test]
fn step_instruction_clocks_vdp_timing() {
    let mut emu = Emulator::new_with_hint(&[0x00], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    for _ in 0..57 {
        emu.step_instruction();
    }

    assert_eq!(emu.bus().vdp().scanline(), 1);
}

#[test]
fn step_frame_generates_psg_audio_samples() {
    let mut emu = Emulator::new_with_hint(&[0x76], 48_000, SystemHint::MasterSystem)
        .expect("emulator should initialize");

    emu.bus_mut()
        .io_write(crate::hardware::constants::IO_PORT_PSG, 0x80);
    emu.bus_mut()
        .io_write(crate::hardware::constants::IO_PORT_PSG, 0x04);
    emu.bus_mut()
        .io_write(crate::hardware::constants::IO_PORT_PSG, 0x90);

    emu.step_frame();
    let samples = emu.drain_audio_samples();

    assert!(
        (1598..=1602).contains(&samples.len()),
        "expected about 800 stereo pairs per frame, got {} samples",
        samples.len()
    );
    assert!(samples.iter().any(|&sample| sample != 0.0));
    assert!(emu.drain_audio_samples().is_empty());
}
