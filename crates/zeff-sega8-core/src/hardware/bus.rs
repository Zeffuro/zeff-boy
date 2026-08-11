use std::cell::RefCell;

use super::apu::{Apu, PSG_CHANNEL_COUNT};
use super::cartridge::{Cartridge, Sega8MapperKind, Sega8System};
use super::constants::{
    FIXED_BOOT_ROM_BYTES, IO_OPEN_BUS_VALUE, IO_PORT_CONTROLLER_1, IO_PORT_CONTROLLER_2,
    IO_PORT_GG_EXT_DATA, IO_PORT_GG_EXT_DIRECTION, IO_PORT_GG_PSG_STEREO,
    IO_PORT_GG_SERIAL_CONTROL, IO_PORT_GG_SERIAL_RX, IO_PORT_GG_SERIAL_TX, IO_PORT_GG_START,
    IO_PORT_H_COUNTER, IO_PORT_PSG, IO_PORT_PSG_MIRROR_MASK, IO_PORT_PSG_MIRROR_VALUE,
    IO_PORT_TMS9918_CONTROL, IO_PORT_TMS9918_DATA, IO_PORT_V_COUNTER, IO_PORT_VDP_CONTROL,
    IO_PORT_VDP_CONTROL_MIRROR_VALUE, IO_PORT_VDP_DATA, IO_PORT_VDP_DATA_MIRROR_VALUE,
    IO_PORT_VDP_MIRROR_MASK, MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT,
    MAPPER_FRAME_CONTROL_CART_RAM_ENABLE, MAPPER_SLOT0_BANK, MAPPER_SLOT1_BANK, MAPPER_SLOT2_BANK,
    SLOT_SIZE, SLOT0_END, SLOT0_START, SLOT1_END, SLOT1_START, SLOT2_END, SLOT2_START,
    SMS_CARTRIDGE_RAM_BANK_SIZE, SMS_CARTRIDGE_RAM_SIZE, SMS_WORK_RAM_SIZE, WORK_RAM_END,
    WORK_RAM_MASK, WORK_RAM_START,
};
use super::input::{ControllerPort, Input};
use super::region::Sega8Region;
use super::serial::GameGearSerial;
use super::timing::Sega8VideoStandard;
use super::vdp::Vdp;

const SAVE_STATE_VERSION_WITH_GG_START: u32 = 2;
const SAVE_STATE_VERSION_WITH_IO_CONTROL: u32 = 4;
const SAVE_STATE_VERSION_WITH_GG_SERIAL: u32 = 6;
const SAVE_STATE_VERSION_WITH_GG_SERIAL_FLAGS: u32 = 7;
const IO_CONTROL_DEFAULT: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegaMapper {
    kind: Sega8MapperKind,
    frame_control: u8,
    slot_banks: [u8; 3],
}

impl Default for SegaMapper {
    fn default() -> Self {
        Self::new(Sega8MapperKind::Sega)
    }
}

impl SegaMapper {
    fn new(kind: Sega8MapperKind) -> Self {
        Self {
            kind,
            frame_control: 0,
            slot_banks: default_slot_banks(kind),
        }
    }

    pub fn kind(self) -> Sega8MapperKind {
        self.kind
    }

    pub fn kind_label(self) -> &'static str {
        self.kind.label()
    }

    pub fn frame_control(self) -> u8 {
        self.frame_control
    }

    pub fn slot_banks(self) -> [u8; 3] {
        self.slot_banks
    }

    pub fn slot2_cartridge_ram_enabled(self) -> bool {
        self.kind == Sega8MapperKind::Sega
            && self.frame_control & MAPPER_FRAME_CONTROL_CART_RAM_ENABLE != 0
    }

    pub fn cartridge_ram_bank(self) -> usize {
        usize::from(self.frame_control & MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT != 0)
    }

    fn reset(&mut self) {
        *self = Self::new(self.kind);
    }

    fn write_sega_register(&mut self, addr: u16, val: u8) {
        match addr {
            MAPPER_FRAME_CONTROL => self.frame_control = val,
            MAPPER_SLOT0_BANK => self.slot_banks[0] = val,
            MAPPER_SLOT1_BANK => self.slot_banks[1] = val,
            MAPPER_SLOT2_BANK => self.slot_banks[2] = val,
            _ => {}
        }
    }

    fn write_codemasters_register(&mut self, addr: u16, val: u8) {
        match addr {
            SLOT0_START..=SLOT0_END => self.slot_banks[0] = val,
            SLOT1_START..=SLOT1_END => self.slot_banks[1] = val,
            SLOT2_START..=SLOT2_END => self.slot_banks[2] = val,
            _ => {}
        }
    }
}

fn default_slot_banks(kind: Sega8MapperKind) -> [u8; 3] {
    match kind {
        Sega8MapperKind::Sega => [0, 1, 2],
        Sega8MapperKind::Codemasters => [0, 1, 0],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuAccessTraceEvent {
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        old_value: u8,
        new_value: u8,
    },
    IoRead {
        port: u8,
        value: u8,
    },
    IoWrite {
        port: u8,
        value: u8,
    },
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub cartridge: Cartridge,
    mapper: SegaMapper,
    work_ram: [u8; SMS_WORK_RAM_SIZE],
    cartridge_ram: [u8; SMS_CARTRIDGE_RAM_SIZE],
    vdp: Vdp,
    apu: Apu,
    input: Input,
    game_gear_serial: GameGearSerial,
    rom_patches: Vec<zeff_emu_common::cheats::CheatPatch>,
    console_region: Sega8Region,
    debug_trace_enabled: bool,
    debug_trace_events: RefCell<Vec<CpuAccessTraceEvent>>,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self::new_with_sample_rate(cartridge, crate::emulator::DEFAULT_SAMPLE_RATE)
    }

    pub fn new_with_sample_rate(cartridge: Cartridge, sample_rate: u32) -> Self {
        Self::new_with_sample_rate_and_video_standard(
            cartridge,
            sample_rate,
            Sega8VideoStandard::default(),
        )
    }

    pub fn new_with_sample_rate_and_video_standard(
        cartridge: Cartridge,
        sample_rate: u32,
        video_standard: Sega8VideoStandard,
    ) -> Self {
        Self::new_with_sample_rate_video_standard_and_region(
            cartridge,
            sample_rate,
            video_standard,
            Sega8Region::default(),
        )
    }

    pub fn new_with_sample_rate_video_standard_and_region(
        cartridge: Cartridge,
        sample_rate: u32,
        video_standard: Sega8VideoStandard,
        console_region: Sega8Region,
    ) -> Self {
        let mapper = SegaMapper::new(cartridge.mapper_kind());
        Self {
            cartridge,
            mapper,
            work_ram: [0; SMS_WORK_RAM_SIZE],
            cartridge_ram: [0; SMS_CARTRIDGE_RAM_SIZE],
            vdp: Vdp::new_with_video_standard(video_standard),
            apu: Apu::new_with_sample_rate(sample_rate),
            input: Input::new(),
            game_gear_serial: GameGearSerial::new(),
            rom_patches: Vec::new(),
            console_region,
            debug_trace_enabled: false,
            debug_trace_events: RefCell::new(Vec::new()),
        }
    }

    pub fn mapper(&self) -> SegaMapper {
        self.mapper
    }

    pub fn work_ram(&self) -> &[u8; SMS_WORK_RAM_SIZE] {
        &self.work_ram
    }

    pub fn cartridge_ram(&self) -> &[u8; SMS_CARTRIDGE_RAM_SIZE] {
        &self.cartridge_ram
    }

    pub fn load_cartridge_ram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if bytes.len() != self.cartridge_ram.len() {
            anyhow::bail!(
                "Sega 8-bit cartridge RAM size mismatch: got {} bytes, expected {}",
                bytes.len(),
                self.cartridge_ram.len()
            );
        }
        self.cartridge_ram.copy_from_slice(bytes);
        Ok(())
    }

    pub fn vdp(&self) -> &Vdp {
        &self.vdp
    }

    pub fn vdp_mut(&mut self) -> &mut Vdp {
        &mut self.vdp
    }

    pub fn video_standard(&self) -> Sega8VideoStandard {
        self.vdp.video_standard()
    }

    pub fn set_video_standard(&mut self, video_standard: Sega8VideoStandard) {
        self.vdp.set_video_standard(video_standard);
    }

    pub fn console_region(&self) -> Sega8Region {
        self.console_region
    }

    pub fn set_console_region(&mut self, console_region: Sega8Region) {
        self.console_region = console_region;
    }

    pub fn apu(&self) -> &Apu {
        &self.apu
    }

    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    pub fn input(&self) -> &Input {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    pub fn game_gear_serial(&self) -> &GameGearSerial {
        &self.game_gear_serial
    }

    pub fn game_gear_serial_mut(&mut self) -> &mut GameGearSerial {
        &mut self.game_gear_serial
    }

    pub fn sync_game_gear_link_peer(&mut self, peer: &mut Self) {
        if self.cartridge.system() == Sega8System::GameGear
            && peer.cartridge.system() == Sega8System::GameGear
        {
            self.game_gear_serial
                .exchange_with_peer(&mut peer.game_gear_serial);
        }
    }

    pub fn clear_rom_patches(&mut self) {
        self.rom_patches.clear();
    }

    pub fn add_rom_patch(&mut self, patch: zeff_emu_common::cheats::CheatPatch) {
        self.rom_patches.push(patch);
    }

    pub fn rom_patches(&self) -> &[zeff_emu_common::cheats::CheatPatch] {
        &self.rom_patches
    }

    pub fn reset(&mut self) {
        self.mapper.reset();
        self.work_ram.fill(0);
        self.vdp.reset();
        self.apu.reset();
        self.input.reset();
        self.game_gear_serial.reset();
        self.rom_patches.clear();
        self.debug_trace_enabled = false;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        self.vdp.step_cycles(cycles);
        self.apu.step_cycles(cycles);
    }

    pub fn drain_audio_samples_into(&mut self, out: &mut Vec<f32>) {
        self.apu.drain_audio_samples_into(out);
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.apu.set_sample_rate(sample_rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.apu.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.apu.set_channel_mutes(mutes);
    }

    pub fn maskable_interrupt_pending(&self) -> bool {
        self.vdp.interrupt_pending()
    }

    pub fn begin_cpu_access_trace(&mut self) {
        self.debug_trace_enabled = true;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub fn drain_cpu_access_trace(&mut self) -> Vec<CpuAccessTraceEvent> {
        self.debug_trace_enabled = false;
        std::mem::take(&mut *self.debug_trace_events.borrow_mut())
    }

    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        w.write_u8(mapper_kind_to_byte(self.mapper.kind));
        w.write_u8(self.mapper.frame_control);
        for bank in self.mapper.slot_banks {
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
        self.mapper.kind = mapper_kind;
        self.mapper.frame_control = r.read_u8()?;
        for bank in &mut self.mapper.slot_banks {
            *bank = r.read_u8()?;
        }
        read_fixed_vec(r, &mut self.work_ram, SMS_WORK_RAM_SIZE, "work RAM")?;
        read_fixed_vec(
            r,
            &mut self.cartridge_ram,
            SMS_CARTRIDGE_RAM_SIZE,
            "cartridge RAM",
        )?;
        self.vdp.read_state(r)?;
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
            self.game_gear_serial
                .read_state(r, version >= SAVE_STATE_VERSION_WITH_GG_SERIAL_FLAGS)?;
        } else {
            self.game_gear_serial.reset();
        }
        self.debug_trace_enabled = false;
        self.debug_trace_events.borrow_mut().clear();
        Ok(())
    }

    pub fn cpu_read(&self, addr: u16) -> u8 {
        let raw = self.cpu_read_raw(addr);
        let value = self.apply_rom_patch(addr, raw);
        self.record_cpu_read(addr, value);
        value
    }

    fn cpu_read_raw(&self, addr: u16) -> u8 {
        match addr {
            SLOT0_START..=SLOT0_END => {
                if self.mapper.kind() == Sega8MapperKind::Sega && addr < FIXED_BOOT_ROM_BYTES {
                    self.cartridge.read_bank(0, addr)
                } else {
                    self.cartridge
                        .read_bank(self.mapper.slot_banks[0], addr % SLOT_SIZE)
                }
            }
            SLOT1_START..=SLOT1_END => self
                .cartridge
                .read_bank(self.mapper.slot_banks[1], addr.wrapping_sub(SLOT1_START)),
            SLOT2_START..=SLOT2_END => {
                if self.mapper.slot2_cartridge_ram_enabled() {
                    let offset = self.cartridge_ram_offset(addr);
                    self.cartridge_ram[offset]
                } else {
                    self.cartridge
                        .read_bank(self.mapper.slot_banks[2], addr.wrapping_sub(SLOT2_START))
                }
            }
            WORK_RAM_START..=WORK_RAM_END => self.work_ram[(addr & WORK_RAM_MASK) as usize],
        }
    }

    fn apply_rom_patch(&self, addr: u16, raw: u8) -> u8 {
        if self.rom_patches.is_empty() || !self.is_rom_read_address(addr) {
            return raw;
        }

        for patch in &self.rom_patches {
            match *patch {
                zeff_emu_common::cheats::CheatPatch::RomWrite { address, value }
                    if address == addr =>
                {
                    return value.resolve_with_current(raw);
                }
                zeff_emu_common::cheats::CheatPatch::RomWriteIfEquals {
                    address,
                    value,
                    compare,
                } if address == addr && compare.matches(raw) => {
                    return value.resolve_with_current(raw);
                }
                _ => {}
            }
        }

        raw
    }

    fn is_rom_read_address(&self, addr: u16) -> bool {
        matches!(addr, SLOT0_START..=SLOT1_END)
            || matches!(addr, SLOT2_START..=SLOT2_END) && !self.mapper.slot2_cartridge_ram_enabled()
    }

    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        let old_value = self.cpu_read_raw(addr);
        match addr {
            SLOT0_START..=SLOT2_END if self.mapper.kind() == Sega8MapperKind::Codemasters => {
                self.mapper.write_codemasters_register(addr, val);
            }
            SLOT2_START..=SLOT2_END if self.mapper.slot2_cartridge_ram_enabled() => {
                let offset = self.cartridge_ram_offset(addr);
                self.cartridge_ram[offset] = val;
            }
            WORK_RAM_START..=WORK_RAM_END => {
                self.work_ram[(addr & WORK_RAM_MASK) as usize] = val;
                self.mapper.write_sega_register(addr, val);
            }
            _ => {}
        }
        self.record_cpu_write(addr, old_value, val);
    }

    fn cartridge_ram_offset(&self, addr: u16) -> usize {
        self.mapper.cartridge_ram_bank() * SMS_CARTRIDGE_RAM_BANK_SIZE
            + usize::from(addr.wrapping_sub(SLOT2_START))
    }

    pub fn io_read(&mut self, port: u8) -> u8 {
        let value = match port {
            IO_PORT_GG_START if self.cartridge.system() == Sega8System::GameGear => {
                self.input.read_game_gear_start(self.console_region)
            }
            IO_PORT_GG_EXT_DATA if self.cartridge.system() == Sega8System::GameGear => {
                self.game_gear_serial.read_ext_data()
            }
            IO_PORT_GG_EXT_DIRECTION if self.cartridge.system() == Sega8System::GameGear => {
                self.game_gear_serial.ext_direction()
            }
            IO_PORT_GG_SERIAL_TX if self.cartridge.system() == Sega8System::GameGear => {
                self.game_gear_serial.tx_data()
            }
            IO_PORT_GG_SERIAL_RX if self.cartridge.system() == Sega8System::GameGear => {
                self.game_gear_serial.read_rx_data()
            }
            IO_PORT_GG_SERIAL_CONTROL if self.cartridge.system() == Sega8System::GameGear => {
                self.game_gear_serial.read_status()
            }
            IO_PORT_V_COUNTER | IO_PORT_H_COUNTER => self.read_counter_port(port),
            IO_PORT_VDP_DATA | IO_PORT_TMS9918_DATA => self.vdp.read_data(),
            IO_PORT_VDP_CONTROL | IO_PORT_TMS9918_CONTROL => self.vdp.read_status(),
            IO_PORT_CONTROLLER_1 => self
                .input
                .read_controller_for_bus(ControllerPort::One, self.console_region),
            IO_PORT_CONTROLLER_2 => self
                .input
                .read_controller_for_bus(ControllerPort::Two, self.console_region),
            _ => {
                if is_counter_mirror(port) {
                    self.read_counter_port(port)
                } else if is_vdp_data_mirror(port) {
                    self.vdp.read_data()
                } else if is_vdp_control_mirror(port) {
                    self.vdp.read_status()
                } else if let Some(controller_port) =
                    controller_port_mirror(self.cartridge.system(), port)
                {
                    self.input
                        .read_controller_for_bus(controller_port, self.console_region)
                } else {
                    IO_OPEN_BUS_VALUE
                }
            }
        };
        self.record_io_read(port, value);
        value
    }

    pub fn io_write(&mut self, port: u8, val: u8) {
        if self.write_game_gear_specific_port(port, val) {
            self.record_io_write(port, val);
            return;
        }

        if is_low_io_control_mirror(port) {
            self.input.set_io_control(val);
        } else if port == IO_PORT_PSG || is_psg_write_mirror(port) {
            self.apu.write_data(val);
        } else if port == IO_PORT_VDP_DATA
            || port == IO_PORT_TMS9918_DATA
            || is_vdp_data_mirror(port)
        {
            self.vdp.write_data(val);
        } else if port == IO_PORT_VDP_CONTROL
            || port == IO_PORT_TMS9918_CONTROL
            || is_vdp_control_mirror(port)
        {
            self.vdp.write_control(val);
        }
        self.record_io_write(port, val);
    }

    fn read_counter_port(&self, port: u8) -> u8 {
        if port & 1 == 0 {
            self.vdp.v_counter()
        } else {
            self.vdp.h_counter()
        }
    }

    fn write_game_gear_specific_port(&mut self, port: u8, val: u8) -> bool {
        if self.cartridge.system() != Sega8System::GameGear {
            return false;
        }
        match port {
            IO_PORT_GG_EXT_DATA => self.game_gear_serial.write_ext_data(val),
            IO_PORT_GG_EXT_DIRECTION => self.game_gear_serial.write_ext_direction(val),
            IO_PORT_GG_SERIAL_TX => self.game_gear_serial.write_tx_data(val),
            IO_PORT_GG_SERIAL_RX => {}
            IO_PORT_GG_SERIAL_CONTROL => self.game_gear_serial.write_control(val),
            IO_PORT_GG_PSG_STEREO => self.apu.write_stereo_control(val),
            _ => return false,
        }
        true
    }

    fn record_cpu_read(&self, addr: u16, value: u8) {
        if self.debug_trace_enabled {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Read { addr, value });
        }
    }

    fn record_cpu_write(&self, addr: u16, old_value: u8, new_value: u8) {
        if self.debug_trace_enabled {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Write {
                    addr,
                    old_value,
                    new_value,
                });
        }
    }

    fn record_io_read(&self, port: u8, value: u8) {
        if self.debug_trace_enabled {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::IoRead { port, value });
        }
    }

    fn record_io_write(&self, port: u8, value: u8) {
        if self.debug_trace_enabled {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::IoWrite { port, value });
        }
    }
}

fn is_vdp_data_mirror(port: u8) -> bool {
    port & IO_PORT_VDP_MIRROR_MASK == IO_PORT_VDP_DATA_MIRROR_VALUE
}

fn is_vdp_control_mirror(port: u8) -> bool {
    port & IO_PORT_VDP_MIRROR_MASK == IO_PORT_VDP_CONTROL_MIRROR_VALUE
}

fn is_counter_mirror(port: u8) -> bool {
    port & IO_PORT_PSG_MIRROR_MASK == IO_PORT_PSG_MIRROR_VALUE
}

fn is_psg_write_mirror(port: u8) -> bool {
    port & IO_PORT_PSG_MIRROR_MASK == IO_PORT_PSG_MIRROR_VALUE
}

fn is_low_io_control_mirror(port: u8) -> bool {
    port < 0x40 && port & 1 != 0
}

fn controller_port_mirror(system: Sega8System, port: u8) -> Option<ControllerPort> {
    if port < 0xC0 {
        return None;
    }

    match system {
        Sega8System::GameGear => match port {
            0xC0 | 0xDC => Some(ControllerPort::One),
            0xC1 | 0xDD => Some(ControllerPort::Two),
            _ => None,
        },
        Sega8System::MasterSystem | Sega8System::Sg1000 => {
            if port & 1 == 0 {
                Some(ControllerPort::One)
            } else {
                Some(ControllerPort::Two)
            }
        }
    }
}

fn mapper_kind_to_byte(kind: Sega8MapperKind) -> u8 {
    match kind {
        Sega8MapperKind::Sega => 0,
        Sega8MapperKind::Codemasters => 1,
    }
}

fn byte_to_mapper_kind(value: u8) -> anyhow::Result<Sega8MapperKind> {
    match value {
        0 => Ok(Sega8MapperKind::Sega),
        1 => Ok(Sega8MapperKind::Codemasters),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::{Sega8MapperKind, SystemHint};
    use crate::hardware::constants::{
        CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, IO_PORT_CONTROL, IO_PORT_GG_EXT_DATA,
        IO_PORT_GG_EXT_DIRECTION, IO_PORT_GG_SERIAL_CONTROL, IO_PORT_GG_SERIAL_RX,
        IO_PORT_GG_SERIAL_TX, ROM_BANK_SIZE,
    };

    const CODEMASTERS_TEST_HEADER_BANK_COUNT: usize = 0x00;
    const CODEMASTERS_TEST_HEADER_DAY: usize = 0x01;
    const CODEMASTERS_TEST_HEADER_MONTH: usize = 0x02;
    const CODEMASTERS_TEST_HEADER_YEAR: usize = 0x03;
    const CODEMASTERS_TEST_HEADER_HOUR: usize = 0x04;
    const CODEMASTERS_TEST_HEADER_MINUTE: usize = 0x05;
    const CODEMASTERS_TEST_HEADER_CHECKSUM: usize = 0x06;
    const CODEMASTERS_TEST_HEADER_COMPLEMENT: usize = 0x08;
    const CODEMASTERS_TEST_HEADER_ZERO_PADDING_START: usize = 0x0A;

    fn banked_rom(bank_count: usize) -> Vec<u8> {
        let mut rom = vec![0; bank_count * ROM_BANK_SIZE];
        for bank in 0..bank_count {
            rom[bank * ROM_BANK_SIZE..(bank + 1) * ROM_BANK_SIZE].fill(bank as u8);
        }
        rom
    }

    fn codemasters_banked_rom(bank_count: usize) -> Vec<u8> {
        let mut rom = banked_rom(bank_count);
        let offset = CODEMASTERS_HEADER_OFFSET;
        assert!(rom.len() >= offset + CODEMASTERS_HEADER_SIZE);
        rom[offset + CODEMASTERS_TEST_HEADER_BANK_COUNT] = bank_count as u8;
        rom[offset + CODEMASTERS_TEST_HEADER_DAY] = 0x31;
        rom[offset + CODEMASTERS_TEST_HEADER_MONTH] = 0x08;
        rom[offset + CODEMASTERS_TEST_HEADER_YEAR] = 0x93;
        rom[offset + CODEMASTERS_TEST_HEADER_HOUR] = 0x10;
        rom[offset + CODEMASTERS_TEST_HEADER_MINUTE] = 0x59;
        rom[offset + CODEMASTERS_TEST_HEADER_CHECKSUM
            ..offset + CODEMASTERS_TEST_HEADER_CHECKSUM + 2]
            .copy_from_slice(&0x1234u16.to_le_bytes());
        rom[offset + CODEMASTERS_TEST_HEADER_COMPLEMENT
            ..offset + CODEMASTERS_TEST_HEADER_COMPLEMENT + 2]
            .copy_from_slice(&0xEDCCu16.to_le_bytes());
        rom[offset + CODEMASTERS_TEST_HEADER_ZERO_PADDING_START..offset + CODEMASTERS_HEADER_SIZE]
            .fill(0);
        rom
    }

    fn bus_with_banked_rom(bank_count: usize) -> Bus {
        let cart = Cartridge::load_with_hint(&banked_rom(bank_count), SystemHint::MasterSystem)
            .expect("banked ROM should load");
        Bus::new(cart)
    }

    fn game_gear_bus_with_banked_rom(bank_count: usize) -> Bus {
        let cart = Cartridge::load_with_hint(&banked_rom(bank_count), SystemHint::GameGear)
            .expect("banked ROM should load");
        Bus::new(cart)
    }

    fn bus_with_codemasters_banked_rom(bank_count: usize) -> Bus {
        let cart = Cartridge::load_with_hint(
            &codemasters_banked_rom(bank_count),
            SystemHint::MasterSystem,
        )
        .expect("Codemasters banked ROM should load");
        Bus::new(cart)
    }

    #[test]
    fn default_mapper_exposes_first_three_rom_banks() {
        let bus = bus_with_banked_rom(4);

        assert_eq!(bus.cpu_read(0x0000), 0);
        assert_eq!(bus.cpu_read(0x0400), 0);
        assert_eq!(bus.cpu_read(0x4000), 1);
        assert_eq!(bus.cpu_read(0x8000), 2);
    }

    #[test]
    fn mapper_registers_switch_slots_but_keep_first_kilobyte_fixed() {
        let mut bus = bus_with_banked_rom(4);

        bus.cpu_write(MAPPER_SLOT0_BANK, 3);
        bus.cpu_write(MAPPER_SLOT1_BANK, 2);
        bus.cpu_write(MAPPER_SLOT2_BANK, 1);

        assert_eq!(bus.cpu_read(0x0000), 0);
        assert_eq!(bus.cpu_read(0x0400), 3);
        assert_eq!(bus.cpu_read(0x4000), 2);
        assert_eq!(bus.cpu_read(0x8000), 1);
        assert_eq!(bus.mapper().slot_banks(), [3, 2, 1]);
    }

    #[test]
    fn standard_sega_mapper_ignores_codemasters_register_addresses() {
        let mut bus = bus_with_banked_rom(4);

        bus.cpu_write(SLOT2_START, 3);

        assert_eq!(bus.mapper().kind(), Sega8MapperKind::Sega);
        assert_eq!(bus.mapper().slot_banks(), [0, 1, 2]);
        assert_eq!(bus.cpu_read(SLOT2_START), 2);
    }

    #[test]
    fn codemasters_mapper_uses_detected_initial_banks() {
        let bus = bus_with_codemasters_banked_rom(4);

        assert_eq!(bus.mapper().kind(), Sega8MapperKind::Codemasters);
        assert_eq!(bus.mapper().kind_label(), "codemasters");
        assert_eq!(bus.mapper().slot_banks(), [0, 1, 0]);
        assert_eq!(bus.cpu_read(SLOT0_START), 0);
        assert_eq!(bus.cpu_read(SLOT1_START), 1);
        assert_eq!(bus.cpu_read(SLOT2_START), 0);
    }

    #[test]
    fn codemasters_mapper_switches_all_three_slots_without_fixed_boot_window() {
        let mut bus = bus_with_codemasters_banked_rom(4);

        bus.cpu_write(SLOT0_START, 3);
        bus.cpu_write(SLOT1_START + 1, 2);
        bus.cpu_write(SLOT2_START + 2, 1);

        assert_eq!(bus.mapper().slot_banks(), [3, 2, 1]);
        assert_eq!(bus.cpu_read(SLOT0_START), 3);
        assert_eq!(bus.cpu_read(SLOT1_START), 2);
        assert_eq!(bus.cpu_read(SLOT2_START), 1);
    }

    #[test]
    fn work_ram_is_mirrored_and_mapper_registers_are_ram_backed() {
        let mut bus = bus_with_banked_rom(4);

        bus.cpu_write(0xC123, 0x5A);
        assert_eq!(bus.cpu_read(0xC123), 0x5A);
        assert_eq!(bus.cpu_read(0xE123), 0x5A);

        bus.cpu_write(MAPPER_FRAME_CONTROL, 0x08);
        assert_eq!(bus.cpu_read(MAPPER_FRAME_CONTROL), 0x08);
        assert_eq!(bus.mapper().frame_control(), 0x08);
    }

    #[test]
    fn sega_mapper_can_map_cartridge_ram_into_slot2() {
        let mut bus = bus_with_banked_rom(4);

        assert_eq!(bus.cpu_read(0x8000), 2);
        bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
        bus.cpu_write(0x8000, 0x5A);

        assert!(bus.mapper().slot2_cartridge_ram_enabled());
        assert_eq!(bus.cpu_read(0x8000), 0x5A);
        assert_eq!(bus.cartridge_ram()[0], 0x5A);

        bus.cpu_write(MAPPER_FRAME_CONTROL, 0);

        assert!(!bus.mapper().slot2_cartridge_ram_enabled());
        assert_eq!(bus.cpu_read(0x8000), 2);
    }

    #[test]
    fn sega_mapper_cartridge_ram_bank_select_switches_slot2_ram_page() {
        let mut bus = bus_with_banked_rom(4);

        bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
        bus.cpu_write(0x8000, 0x11);
        bus.cpu_write(
            MAPPER_FRAME_CONTROL,
            MAPPER_FRAME_CONTROL_CART_RAM_ENABLE | MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT,
        );
        bus.cpu_write(0x8000, 0x22);

        assert_eq!(bus.mapper().cartridge_ram_bank(), 1);
        assert_eq!(bus.cpu_read(0x8000), 0x22);

        bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);

        assert_eq!(bus.mapper().cartridge_ram_bank(), 0);
        assert_eq!(bus.cpu_read(0x8000), 0x11);
    }

    #[test]
    fn rom_patches_apply_to_cpu_rom_reads() {
        let mut bus = bus_with_banked_rom(4);

        assert_eq!(bus.cpu_read(0x4001), 1);
        bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
            address: 0x4001,
            value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
        });

        assert_eq!(bus.cpu_read(0x4001), 0xAA);
        assert_eq!(bus.cpu_read_raw(0x4001), 1);
    }

    #[test]
    fn conditional_rom_patches_compare_unpatched_rom_byte() {
        let mut bus = bus_with_banked_rom(4);

        bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWriteIfEquals {
            address: 0x4001,
            value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
            compare: zeff_emu_common::cheats::CheatValue::Constant(0x02),
        });
        assert_eq!(bus.cpu_read(0x4001), 1);

        bus.clear_rom_patches();
        bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWriteIfEquals {
            address: 0x4001,
            value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
            compare: zeff_emu_common::cheats::CheatValue::Constant(0x01),
        });
        assert_eq!(bus.cpu_read(0x4001), 0xAA);
    }

    #[test]
    fn rom_patches_do_not_override_mapped_cartridge_ram() {
        let mut bus = bus_with_banked_rom(4);

        bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
            address: 0x8000,
            value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
        });
        assert_eq!(bus.cpu_read(0x8000), 0xAA);

        bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
        bus.cpu_write(0x8000, 0x5A);

        assert_eq!(bus.cpu_read(0x8000), 0x5A);
    }

    #[test]
    fn reset_restores_mapper_and_clears_ram() {
        let mut bus = bus_with_banked_rom(4);
        bus.cpu_write(MAPPER_SLOT1_BANK, 3);
        bus.cpu_write(0xC000, 0xAA);
        bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
        bus.cpu_write(0x8000, 0x5A);
        bus.io_write(IO_PORT_PSG, 0x9F);
        bus.io_write(IO_PORT_VDP_CONTROL, 0xE0);
        bus.io_write(IO_PORT_VDP_CONTROL, 0x80);
        bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
            address: 0,
            value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
        });

        bus.reset();

        assert_eq!(bus.mapper().slot_banks(), [0, 1, 2]);
        assert_eq!(bus.cpu_read(0xC000), 0);
        assert_eq!(bus.cartridge_ram()[0], 0x5A);
        assert_eq!(bus.apu().last_write(), None);
        assert_eq!(bus.vdp().registers()[0], 0);
        assert!(bus.rom_patches().is_empty());
    }

    #[test]
    fn reset_preserves_codemasters_mapper_kind_and_default_banks() {
        let mut bus = bus_with_codemasters_banked_rom(4);
        bus.cpu_write(SLOT2_START, 3);

        bus.reset();

        assert_eq!(bus.mapper().kind(), Sega8MapperKind::Codemasters);
        assert_eq!(bus.mapper().slot_banks(), [0, 1, 0]);
        assert_eq!(bus.cpu_read(SLOT2_START), 0);
    }

    #[test]
    fn io_ports_route_to_vdp_psg_and_controllers() {
        let mut bus = bus_with_banked_rom(4);

        bus.io_write(IO_PORT_VDP_CONTROL, 0x34);
        bus.io_write(IO_PORT_VDP_CONTROL, 0x41);
        bus.io_write(IO_PORT_VDP_DATA, 0xAA);
        bus.io_write(IO_PORT_PSG, 0x9F);
        bus.input_mut()
            .set_controller_raw(ControllerPort::One, 0xF7);

        assert_eq!(bus.vdp().vram()[0x0134], 0xAA);
        assert_eq!(bus.apu().last_write(), Some(0x9F));
        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_1), 0xF7);
    }

    #[test]
    fn export_sms_region_detector_reads_back_th_outputs_from_port_3f() {
        let mut bus = bus_with_banked_rom(4);
        bus.set_console_region(Sega8Region::Export);

        bus.io_write(IO_PORT_CONTROL, 0xF5);
        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);

        bus.io_write(IO_PORT_CONTROL, 0x55);
        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);
    }

    #[test]
    fn japanese_sms_region_detector_does_not_read_back_th_outputs() {
        let mut bus = bus_with_banked_rom(4);
        bus.set_console_region(Sega8Region::Japanese);
        bus.input_mut()
            .set_controller_raw(ControllerPort::Two, 0xC0);

        bus.io_write(IO_PORT_CONTROL, 0x55);

        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);
    }

    #[test]
    fn japanese_power_base_converter_region_detector_inverts_th_outputs() {
        let mut bus = bus_with_banked_rom(4);
        bus.set_console_region(Sega8Region::JapanesePowerBaseConverter);

        bus.io_write(IO_PORT_CONTROL, 0xF5);
        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);

        bus.io_write(IO_PORT_CONTROL, 0x55);
        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);
    }

    #[test]
    fn game_gear_start_port_reports_region_bit() {
        let cart = Cartridge::load_with_hint(&[0x00], SystemHint::GameGear)
            .expect("Game Gear ROM should load");
        let mut bus = Bus::new(cart);

        bus.set_console_region(Sega8Region::Export);
        assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x40);

        bus.set_console_region(Sega8Region::Japanese);
        assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x00);

        bus.set_console_region(Sega8Region::JapanesePowerBaseConverter);
        assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x00);
    }

    #[test]
    fn psg_write_mirrors_and_game_gear_stereo_port_are_decoded() {
        let mut bus = bus_with_banked_rom(4);

        bus.io_write(0x40, 0x90);
        bus.io_write(0x7E, 0x80);

        assert_eq!(bus.apu().last_write(), Some(0x80));
        assert_eq!(bus.apu().write_count(), 2);

        let mut gg = game_gear_bus_with_banked_rom(4);
        gg.io_write(IO_PORT_GG_PSG_STEREO, 0x10);

        assert_eq!(gg.apu().stereo_control(), 0x10);
    }

    #[test]
    fn game_gear_serial_status_port_reports_idle_disconnected_link() {
        let mut bus = game_gear_bus_with_banked_rom(4);

        bus.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);

        assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x01, 0);
        assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
        assert_ne!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x04, 0);
    }

    #[test]
    fn game_gear_serial_sync_transfers_pending_tx_to_peer_rx() {
        let mut left = game_gear_bus_with_banked_rom(4);
        let mut right = game_gear_bus_with_banked_rom(4);

        left.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
        right.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
        left.io_write(IO_PORT_GG_SERIAL_TX, 0x5A);
        left.sync_game_gear_link_peer(&mut right);

        assert_eq!(left.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x05, 0);
        assert_ne!(right.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
        assert_eq!(right.io_read(IO_PORT_GG_SERIAL_RX), 0x5A);
        assert_eq!(right.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    }

    #[test]
    fn game_gear_parallel_ext_inputs_can_read_peer_outputs() {
        let mut left = game_gear_bus_with_banked_rom(4);
        let mut right = game_gear_bus_with_banked_rom(4);

        left.io_write(IO_PORT_GG_EXT_DIRECTION, 0x7F);
        right.io_write(IO_PORT_GG_EXT_DIRECTION, 0x00);
        right.io_write(IO_PORT_GG_EXT_DATA, 0x2A);
        left.sync_game_gear_link_peer(&mut right);

        assert_eq!(left.io_read(IO_PORT_GG_EXT_DATA), 0xAA);
    }

    #[test]
    fn sms_low_odd_ports_mirror_io_control_write() {
        let mut bus = bus_with_banked_rom(4);
        bus.set_console_region(Sega8Region::Export);

        bus.io_write(0x01, 0x55);

        assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);
    }

    #[test]
    fn counter_and_controller_read_mirrors_are_decoded() {
        let mut bus = bus_with_banked_rom(4);
        bus.step_cycles(crate::hardware::constants::SMS_SCANLINE_Z80_CYCLES);
        bus.input_mut()
            .set_controller_raw(ControllerPort::One, 0xEE);
        bus.input_mut()
            .set_controller_raw(ControllerPort::Two, 0xDD);

        assert_eq!(bus.io_read(0x40), bus.vdp().v_counter());
        assert_eq!(bus.io_read(0x41), bus.vdp().h_counter());
        assert_eq!(bus.io_read(0xC0), 0xEE);
        assert_eq!(bus.io_read(0xC1), 0xDD);
    }

    #[test]
    fn mirrored_vdp_ports_are_decoded() {
        let mut bus = bus_with_banked_rom(4);

        bus.io_write(0x81, 0x02);
        bus.io_write(0x81, 0x40);
        bus.io_write(0x80, 0x55);

        assert_eq!(bus.vdp().vram()[0x0002], 0x55);
    }

    #[test]
    fn stepping_bus_advances_vdp_timing() {
        let mut bus = bus_with_banked_rom(4);
        bus.io_write(IO_PORT_PSG, 0x90);

        bus.step_cycles(crate::hardware::constants::SMS_SCANLINE_Z80_CYCLES);

        assert_eq!(bus.vdp().scanline(), 1);
        assert_eq!(bus.io_read(IO_PORT_V_COUNTER), 1);
        assert!(bus.apu().buffered_sample_count() > 0);
    }

    #[test]
    fn maskable_interrupt_line_follows_vdp_frame_interrupt() {
        let mut bus = bus_with_banked_rom(4);

        bus.vdp_mut()
            .set_status_bits(crate::hardware::constants::VDP_STATUS_VBLANK);
        assert!(!bus.maskable_interrupt_pending());

        bus.io_write(
            IO_PORT_VDP_CONTROL,
            crate::hardware::constants::VDP_REG1_FRAME_IRQ_ENABLE,
        );
        bus.io_write(
            IO_PORT_VDP_CONTROL,
            crate::hardware::constants::VDP_CONTROL_REGISTER_WRITE_VALUE
                | crate::hardware::constants::VDP_REGISTER_MODE_CONTROL_2 as u8,
        );
        assert!(bus.maskable_interrupt_pending());

        assert_ne!(bus.io_read(IO_PORT_VDP_CONTROL), 0);
        assert!(!bus.maskable_interrupt_pending());
    }
}
