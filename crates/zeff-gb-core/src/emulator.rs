use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::{bus::Bus, cpu::Cpu};
use std::fmt;

mod boot_init;
mod debug_view;
mod public_api;
mod runtime;
mod state_io;

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
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
    pub(crate) debug: DebugController,
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
    use zeff_emu_common::save_ram::SaveRamKind;

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
}
