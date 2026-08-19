use crate::debug::{CallStackEntry, DebugController, OpcodeLog};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::{bus::Bus, cpu::Cpu};
use std::fmt;

mod boot_init;
mod debug_view;
mod public_api;
mod runtime;
mod state_io;

pub use runtime::{FrameSliceCursor, FrameSliceOutcome};

const CYCLES_PER_FRAME_NORMAL: u64 = 70224;
const CYCLES_PER_FRAME_DOUBLE: u64 = 140448;
type RegisterSeed = (u8, u8, u8, u8, u8, u8, u8, u8);

const DMG_POST_BOOT_REGISTERS: RegisterSeed = (0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D);
const CGB_POST_BOOT_REGISTERS: RegisterSeed = (0x11, 0x80, 0x00, 0x00, 0xFF, 0x56, 0x00, 0x0D);

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Box<Bus>,
    pub(crate) header: RomHeader,
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cycle_count: u64,
    pub(crate) frame_count: u64,
    pub(crate) opcode_log: OpcodeLog,
    pub(crate) instruction_trace: zeff_emu_common::debug::InstructionTraceStore,
    pub(crate) call_stack: Vec<CallStackEntry>,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
    pub(crate) debug: DebugController,
    pub(crate) rom_breakpoints: Vec<usize>,
    pub(crate) hit_rom_breakpoint: Option<usize>,
    pub(crate) rom_hash: [u8; 32],
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .field("hardware_mode", &self.hardware_mode)
            .field("hardware_mode_preference", &self.hardware_mode_preference)
            .field("cycle_count", &self.cycle_count)
            .field("frame_count", &self.frame_count)
            .field("last_opcode", &format_args!("{:#04X}", self.last_opcode))
            .field(
                "last_opcode_pc",
                &format_args!("{:#06X}", self.last_opcode_pc),
            )
            .field("opcode_log", &self.opcode_log)
            .field("debug", &self.debug)
            .field("title", &self.header.title)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::WatchType;
    use crate::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
    use zeff_emu_common::save_ram::SaveRamKind;
    use zeff_emu_common::time::{ClockRate, MachineTiming, MasterTicks};

    fn boot_test_rom(cgb: bool) -> Vec<u8> {
        let mut rom = vec![0; 0x8000];
        if cgb {
            rom[0x143] = 0x80;
        }
        rom
    }

    #[test]
    fn boot_rom_overlay_unmaps_and_reset_reenables_it() {
        let rom = boot_test_rom(false);
        let mut boot_rom = vec![0; 0x100];
        boot_rom[0] = 0x31;
        let mut emulator =
            Emulator::from_rom_data_with_boot_rom(&rom, HardwareModePreference::Auto, &boot_rom)
                .unwrap();

        assert_eq!(emulator.cpu.pc, 0);
        assert_eq!(emulator.cpu_peek8(0xFF40) & 0x80, 0);
        assert_eq!(emulator.cpu_peek8(0), 0x31);
        emulator.cpu_write8(crate::hardware::types::constants::BOOT_ROM_DISABLE, 1);
        assert!(!emulator.boot_rom_enabled());
        assert_eq!(emulator.cpu_peek8(0), 0);

        emulator.reset();
        assert!(emulator.boot_rom_enabled());
        assert_eq!(emulator.cpu.pc, 0);
        assert_eq!(emulator.cpu_peek8(0), 0x31);
    }

    #[test]
    fn cgb_boot_rom_leaves_cartridge_header_window_visible() {
        let mut rom = boot_test_rom(true);
        rom[0x100] = 0xCC;
        let mut boot_rom = vec![0; 0x900];
        boot_rom[0] = 0x11;
        boot_rom[0x100] = 0x22;
        boot_rom[0x200] = 0x33;
        let emulator =
            Emulator::from_rom_data_with_boot_rom(&rom, HardwareModePreference::Auto, &boot_rom)
                .unwrap();

        assert_eq!(emulator.cpu_peek8(0), 0x11);
        assert_eq!(emulator.cpu_peek8(0x100), 0xCC);
        assert_eq!(emulator.cpu_peek8(0x200), 0x33);
    }

    #[test]
    fn post_boot_state_does_not_require_boot_rom() {
        let rom = boot_test_rom(false);
        let boot_rom = vec![0; 0x100];
        let mut with_boot =
            Emulator::from_rom_data_with_boot_rom(&rom, HardwareModePreference::Auto, &boot_rom)
                .unwrap();
        let boot_state = with_boot.encode_state().unwrap();

        let mut without_boot = Emulator::new(&rom, 48_000).unwrap();
        assert!(without_boot.load_state(&boot_state).is_err());

        with_boot.cpu_write8(crate::hardware::types::constants::BOOT_ROM_DISABLE, 1);
        let post_boot_state = with_boot.encode_state().unwrap();
        without_boot.load_state(&post_boot_state).unwrap();
        assert!(!without_boot.has_boot_rom());
        assert!(!without_boot.boot_rom_enabled());
    }

    #[test]
    #[ignore = "requires ZEFF_GB_BOOT_ROM and ZEFF_GB_BOOT_TEST_ROM"]
    fn external_boot_rom_reaches_cartridge_entry() {
        let boot_rom = std::fs::read(std::env::var("ZEFF_GB_BOOT_ROM").unwrap()).unwrap();
        let rom = std::fs::read(std::env::var("ZEFF_GB_BOOT_TEST_ROM").unwrap()).unwrap();
        let mut emulator =
            Emulator::from_rom_data_with_boot_rom(&rom, HardwareModePreference::Auto, &boot_rom)
                .unwrap();

        for _ in 0..5_000_000 {
            if !emulator.boot_rom_enabled() {
                break;
            }
            emulator.step_instruction();
        }

        assert!(!emulator.boot_rom_enabled());
        assert!(emulator.cpu.pc >= 0x0100);

        if boot_rom.len() == 0x100 {
            let mut expected = Vec::with_capacity(0xC0);
            for &source in &rom[0x0104..0x0134] {
                for nibble in [source >> 4, source & 0x0F] {
                    let mut expanded = 0;
                    for bit in 0..4 {
                        expanded |= ((nibble >> bit) & 1) * (3 << (bit * 2));
                    }
                    expected.extend_from_slice(&[expanded, 0, expanded, 0]);
                }
            }
            assert_eq!(&emulator.vram_snapshot()[0x10..0x190], expected);
        }
    }

    #[test]
    fn master_timing_round_trips_through_save_state() {
        let rom = vec![0u8; 0x8000];
        let mut emulator = Emulator::new(&rom, 44_100).expect("GB emulator should initialize");

        fn snapshot(machine: &impl MachineTiming) -> zeff_emu_common::time::TimingSnapshot {
            machine.timing_snapshot()
        }

        assert_eq!(snapshot(&emulator).rate(), ClockRate::from_hz(4_194_304));
        assert_eq!(snapshot(&emulator).now(), MasterTicks::ZERO);

        emulator.step_instruction();
        assert_eq!(snapshot(&emulator).now(), MasterTicks::new(4));
        let saved = emulator.encode_state().expect("state should encode");

        emulator.step_instruction();
        assert_eq!(snapshot(&emulator).now(), MasterTicks::new(8));
        emulator.load_state(&saved).expect("state should restore");
        assert_eq!(snapshot(&emulator).now(), MasterTicks::new(4));
    }

    #[test]
    fn public_api_parity_wrappers_load_step_and_roundtrip_state() {
        let rom = vec![0u8; 0x8000];
        let mut emulator = Emulator::new(&rom, 44_100).expect("GB emulator should initialize");

        assert_eq!(emulator.framebuffer_dimensions(), (160, 144));
        assert_eq!(emulator.frame_count(), 0);
        assert_eq!(emulator.save_ram_kind(), SaveRamKind::none());
        assert!(!emulator.has_battery());
        assert_eq!(emulator.system_ram().len(), emulator.wram_snapshot().len());
        assert_eq!(
            emulator.video_ram_snapshot().len(),
            emulator.vram_snapshot().len()
        );
        assert!(emulator.iter_breakpoints().next().is_none());

        emulator.add_breakpoint(emulator.cpu_pc());
        assert_eq!(
            emulator.iter_breakpoints().collect::<Vec<_>>(),
            vec![emulator.cpu_pc()]
        );
        assert_eq!(emulator.debug_hit_breakpoint(), None);

        emulator.add_watchpoint(0xC000, WatchType::Write);
        assert_eq!(emulator.debug_watchpoints().len(), 1);
        emulator.cpu_write8(0xC000, 0x5A);
        assert_eq!(emulator.cpu_peek8(0xC000), 0x5A);
        assert_eq!(
            emulator.debug_hit_watchpoint().map(|hit| hit.new_value),
            Some(0x5A)
        );
        emulator.remove_breakpoint(emulator.cpu_pc());
        emulator.debug_continue();

        emulator.set_input(0x01, 0x01);
        emulator.step_frame();

        assert_eq!(emulator.frame_count(), 1);

        let state = emulator
            .encode_state()
            .expect("GB emulator should encode state");
        emulator
            .load_state(&state)
            .expect("GB emulator should load state");
    }

    #[test]
    fn public_cpu_peek_does_not_enter_cpu_access_trace() {
        let rom = vec![0u8; 0x8000];
        let mut emulator = Emulator::new(&rom, 44_100).expect("GB emulator should initialize");

        emulator.bus.trace_cpu_accesses = true;
        emulator.bus.begin_cpu_access_trace();
        assert_eq!(emulator.cpu_peek8(0x0000), 0x00);

        let mut events = Vec::new();
        emulator
            .bus
            .drain_cpu_access_trace(|event| events.push(event));
        emulator.bus.trace_cpu_accesses = false;

        assert!(events.is_empty());
    }

    #[test]
    fn guest_call_returns_to_suspended_context() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x150..0x153].copy_from_slice(&[0x3E, 0x42, 0xC9]);
        let mut emulator = Emulator::new(&rom, 44_100).unwrap();
        emulator.debug_suspend();
        let pc = emulator.cpu.pc;
        let sp = emulator.cpu.sp;

        assert_eq!(emulator.debug_execute_guest_call(0x150, 10), Ok(2));
        assert_eq!(emulator.cpu.regs.a, 0x42);
        assert_eq!(emulator.cpu.pc, pc);
        assert_eq!(emulator.cpu.sp, sp);
        assert!(matches!(
            emulator.cpu.running,
            crate::hardware::types::CpuState::Suspended
        ));
    }

    #[test]
    fn rom_breakpoint_uses_the_active_physical_bank() {
        let mut rom = vec![0u8; 4 * 0x4000];
        rom[0x147] = 0x19;
        rom[0x148] = 0x01;
        let mut emulator = Emulator::new(&rom, 44_100).unwrap();
        emulator.add_rom_breakpoint(0x8560);
        emulator.add_rom_breakpoint(0x8560);
        assert_eq!(
            emulator.iter_rom_breakpoints().collect::<Vec<_>>(),
            vec![0x8560]
        );

        emulator.write_byte(0x2000, 2);
        emulator.cpu.pc = 0x4560;
        emulator.step_instruction();
        assert_eq!(emulator.debug_hit_rom_breakpoint(), Some(0x8560));

        emulator.debug_continue();
        emulator.write_byte(0x2000, 1);
        emulator.cpu.pc = 0x4560;
        emulator.step_instruction();
        assert_eq!(emulator.debug_hit_rom_breakpoint(), None);
        assert!(!matches!(
            emulator.cpu.running,
            crate::hardware::types::CpuState::Suspended
        ));
    }

    #[test]
    fn opcode_history_keeps_the_executed_physical_bank() {
        let mut rom = vec![0u8; 4 * 0x4000];
        rom[0x147] = 0x19;
        rom[0x148] = 0x01;
        let mut emulator = Emulator::new(&rom, 44_100).unwrap();
        emulator.set_opcode_log_enabled(true);

        emulator.write_byte(0x2000, 2);
        emulator.cpu.pc = 0x4560;
        emulator.step_instruction();
        emulator.write_byte(0x2000, 1);
        emulator.cpu.pc = 0x4560;
        emulator.step_instruction();

        let history = emulator.recent_opcodes(2);
        assert_eq!(history[0].3, Some(0x4560));
        assert_eq!(history[1].3, Some(0x8560));
    }

    #[test]
    fn game_boy_link_peer_sync_exchanges_internal_and_external_transfer_bytes() {
        let rom = vec![0u8; 0x8000];
        let mut left = Emulator::new(&rom, 44_100).expect("left GB emulator should initialize");
        let mut right = Emulator::new(&rom, 44_100).expect("right GB emulator should initialize");

        left.sync_game_boy_link_peer(&mut right);

        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);
        right.write_byte(SERIAL_SC, 0x80);

        left.step_frame();
        right.step_frame();

        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0x80);

        left.sync_game_boy_link_peer(&mut right);

        assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(right.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_link_peer_sync_completes_unanswered_master_with_peer_output_byte() {
        let rom = vec![0u8; 0x8000];
        let mut left = Emulator::new(&rom, 44_100).expect("left GB emulator should initialize");
        let mut right = Emulator::new(&rom, 44_100).expect("right GB emulator should initialize");

        left.sync_game_boy_link_peer(&mut right);

        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);

        left.step_frame();
        left.sync_game_boy_link_peer(&mut right);

        assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(right.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0);
    }

    #[test]
    fn game_boy_remote_link_state_matches_local_sync_result() {
        let rom = vec![0u8; 0x8000];
        let mut left = Emulator::new(&rom, 44_100).expect("left GB emulator should initialize");
        let mut right = Emulator::new(&rom, 44_100).expect("right GB emulator should initialize");

        left.set_game_boy_link_peer_present(true);
        right.set_game_boy_link_peer_present(true);
        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);
        right.write_byte(SERIAL_SC, 0x80);

        left.step_frame();
        right.step_frame();

        let left_state = left.game_boy_link_state();
        let right_state = right.game_boy_link_state();
        left.sync_game_boy_remote_link_peer(right_state);
        right.sync_game_boy_remote_link_peer(left_state);

        assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(right.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }
}
