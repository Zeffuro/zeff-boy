use super::{Bus, CpuAccessTraceMode, IO_CONTROL_DEFAULT, MEMORY_CONTROL_DEFAULT, SegaMapper};
use crate::hardware::cartridge::Sega8MapperKind;
use crate::hardware::constants::{SMS_CARTRIDGE_RAM_SIZE, SMS_WORK_RAM_SIZE};
use crate::hardware::input::ControllerPort;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;

const SAVE_STATE_VERSION_WITH_GG_START: u32 = 2;
const SAVE_STATE_VERSION_WITH_IO_CONTROL: u32 = 4;
const SAVE_STATE_VERSION_WITH_GG_SERIAL: u32 = 6;
const SAVE_STATE_VERSION_WITH_GG_SERIAL_FLAGS: u32 = 7;
const SAVE_STATE_VERSION_WITH_MEMORY_CONTROL: u32 = 8;
const SAVE_STATE_VERSION_WITH_GG_SERIAL_TIMING: u32 = 9;
const SAVE_STATE_VERSION_WITH_VDP_CRAM_LATCH: u32 = 10;
const SAVE_STATE_VERSION_WITH_VDP_SCANLINE_DISPLAY: u32 = 11;

impl Bus {
    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        w.write_u8(mapper_kind_to_byte(self.mapper.kind()));
        w.write_u8(self.mapper.frame_control());
        for bank in self.mapper.slot_banks() {
            w.write_u8(bank);
        }
        w.write_vec(&self.work_ram);
        w.write_vec(&self.cartridge_ram);
        self.vdp.write_state(w);
        self.apu.write_state(w);
        w.write_u8(self.input.read_controller(ControllerPort::One));
        w.write_u8(self.input.read_controller(ControllerPort::Two));
        w.write_bool(self.input.game_gear_start_pressed());
        w.write_u8(self.input.io_control());
        self.game_gear_serial.write_state(w);
        w.write_u8(self.memory_control);
        w.write_u8(self.vdp.gg_cram_latch_state());
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
        version: u32,
    ) -> anyhow::Result<()> {
        let mapper_kind = byte_to_mapper_kind(r.read_u8()?)?;
        if mapper_kind != self.cartridge.mapper_kind() {
            anyhow::bail!(
                "Sega 8-bit save-state mapper mismatch: state={} current={}",
                mapper_kind.label(),
                self.cartridge.mapper_kind().label()
            );
        }
        let frame_control = r.read_u8()?;
        let mut slot_banks = [0; 3];
        for bank in &mut slot_banks {
            *bank = r.read_u8()?;
        }
        self.mapper = SegaMapper::from_state(mapper_kind, frame_control, slot_banks);
        read_fixed_vec(r, &mut self.work_ram, SMS_WORK_RAM_SIZE, "work RAM")?;
        read_fixed_vec(
            r,
            &mut self.cartridge_ram,
            SMS_CARTRIDGE_RAM_SIZE,
            "cartridge RAM",
        )?;
        self.vdp
            .read_state(r, version >= SAVE_STATE_VERSION_WITH_VDP_SCANLINE_DISPLAY)?;
        self.apu.read_state(r)?;
        self.input
            .set_controller_raw(ControllerPort::One, r.read_u8()?);
        self.input
            .set_controller_raw(ControllerPort::Two, r.read_u8()?);
        let game_gear_start_pressed = if version >= SAVE_STATE_VERSION_WITH_GG_START {
            r.read_bool()?
        } else {
            false
        };
        self.input
            .set_game_gear_start_pressed(game_gear_start_pressed);
        let io_control = if version >= SAVE_STATE_VERSION_WITH_IO_CONTROL {
            r.read_u8()?
        } else {
            IO_CONTROL_DEFAULT
        };
        self.input.set_io_control(io_control);
        if version >= SAVE_STATE_VERSION_WITH_GG_SERIAL {
            self.game_gear_serial.read_state(
                r,
                version >= SAVE_STATE_VERSION_WITH_GG_SERIAL_FLAGS,
                version >= SAVE_STATE_VERSION_WITH_GG_SERIAL_TIMING,
            )?;
        } else {
            self.game_gear_serial.reset();
        }
        self.memory_control = if version >= SAVE_STATE_VERSION_WITH_MEMORY_CONTROL {
            r.read_u8()?
        } else {
            MEMORY_CONTROL_DEFAULT
        };
        self.vdp
            .set_gg_cram_latch_state(if version >= SAVE_STATE_VERSION_WITH_VDP_CRAM_LATCH {
                r.read_u8()?
            } else {
                0
            });
        self.debug_trace_mode = CpuAccessTraceMode::None;
        self.debug_trace_events.borrow_mut().clear();
        self.audio_trace
            .invalidate(AudioTraceInvalidation::StateRestore);
        Ok(())
    }
}

fn mapper_kind_to_byte(kind: Sega8MapperKind) -> u8 {
    match kind {
        Sega8MapperKind::Sega => 0,
        Sega8MapperKind::Codemasters => 1,
        Sega8MapperKind::Korean => 2,
        Sega8MapperKind::Msx => 3,
        Sega8MapperKind::Nemesis => 4,
        Sega8MapperKind::Janggun => 5,
    }
}

fn byte_to_mapper_kind(value: u8) -> anyhow::Result<Sega8MapperKind> {
    match value {
        0 => Ok(Sega8MapperKind::Sega),
        1 => Ok(Sega8MapperKind::Codemasters),
        2 => Ok(Sega8MapperKind::Korean),
        3 => Ok(Sega8MapperKind::Msx),
        4 => Ok(Sega8MapperKind::Nemesis),
        5 => Ok(Sega8MapperKind::Janggun),
        _ => anyhow::bail!("invalid Sega 8-bit mapper tag in save-state: {value}"),
    }
}

fn read_fixed_vec(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
    out: &mut [u8],
    expected_len: usize,
    label: &str,
) -> anyhow::Result<()> {
    let bytes = r.read_vec(expected_len)?;
    if bytes.len() != expected_len {
        anyhow::bail!(
            "Sega 8-bit save-state {label} size mismatch: expected {expected_len}, got {}",
            bytes.len()
        );
    }
    out.copy_from_slice(&bytes);
    Ok(())
}
