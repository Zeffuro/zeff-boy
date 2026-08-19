use std::cell::RefCell;

use super::apu::{Apu, PSG_CHANNEL_COUNT};
use super::cartridge::{Cartridge, Sega8MapperKind, Sega8System};
use super::constants::{
    FIXED_BOOT_ROM_BYTES, IO_OPEN_BUS_VALUE, IO_PORT_CONTROLLER_1, IO_PORT_CONTROLLER_2,
    IO_PORT_GG_EXT_DATA, IO_PORT_GG_EXT_DIRECTION, IO_PORT_GG_PSG_STEREO,
    IO_PORT_GG_SERIAL_CONTROL, IO_PORT_GG_SERIAL_RX, IO_PORT_GG_SERIAL_TX, IO_PORT_GG_START,
    IO_PORT_H_COUNTER, IO_PORT_MEMORY_CONTROL, IO_PORT_PSG, IO_PORT_PSG_MIRROR_MASK,
    IO_PORT_PSG_MIRROR_VALUE, IO_PORT_TMS9918_CONTROL, IO_PORT_TMS9918_DATA, IO_PORT_V_COUNTER,
    IO_PORT_VDP_CONTROL, IO_PORT_VDP_CONTROL_MIRROR_VALUE, IO_PORT_VDP_DATA,
    IO_PORT_VDP_DATA_MIRROR_VALUE, IO_PORT_VDP_MIRROR_MASK, ROM_PAGE_8K_SIZE, SG_WORK_RAM_MASK,
    SG_WORK_RAM_SIZE, SLOT_SIZE, SLOT0_END, SLOT0_START, SLOT1_END, SLOT1_START, SLOT2_END,
    SLOT2_START, SMS_CARTRIDGE_RAM_SIZE, SMS_WORK_RAM_SIZE, WORK_RAM_END, WORK_RAM_MASK,
    WORK_RAM_START,
};
use super::input::{ControllerPort, Input};
pub use super::mapper::SegaMapper;
use super::region::Sega8Region;
use super::serial::GameGearSerial;
use super::timing::Sega8VideoStandard;
use super::vdp::{Mode4ColorMode, Vdp};
pub use zeff_emu_common::debug::BusAccessEvent as CpuAccessTraceEvent;
use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};

const SAVE_STATE_VERSION_WITH_GG_START: u32 = 2;
const SAVE_STATE_VERSION_WITH_IO_CONTROL: u32 = 4;
const SAVE_STATE_VERSION_WITH_GG_SERIAL: u32 = 6;
const SAVE_STATE_VERSION_WITH_GG_SERIAL_FLAGS: u32 = 7;
const SAVE_STATE_VERSION_WITH_MEMORY_CONTROL: u32 = 8;
const SAVE_STATE_VERSION_WITH_GG_SERIAL_TIMING: u32 = 9;
const SAVE_STATE_VERSION_WITH_VDP_CRAM_LATCH: u32 = 10;
const SAVE_STATE_VERSION_WITH_VDP_SCANLINE_DISPLAY: u32 = 11;
const IO_CONTROL_DEFAULT: u8 = 0xFF;
const MEMORY_CONTROL_DEFAULT: u8 = 0x00;
const MEMORY_CONTROL_IO_DISABLE: u8 = 1 << 2;
const MEMORY_CONTROL_BOOT_ROM_DISABLE: u8 = 1 << 3;
const MEMORY_CONTROL_WORK_RAM_DISABLE: u8 = 1 << 4;
const MEMORY_CONTROL_CARTRIDGE_DISABLE: u8 = 1 << 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CpuAccessTraceMode {
    #[default]
    None,
    Writes,
    All,
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub cartridge: Cartridge,
    boot_rom: Option<Box<[u8]>>,
    mapper: SegaMapper,
    sg_type_b_ram_extension: bool,
    work_ram: [u8; SMS_WORK_RAM_SIZE],
    cartridge_ram: [u8; SMS_CARTRIDGE_RAM_SIZE],
    vdp: Vdp,
    apu: Apu,
    input: Input,
    game_gear_serial: GameGearSerial,
    memory_control: u8,
    rom_patches: Vec<zeff_emu_common::cheats::CheatPatch>,
    console_region: Sega8Region,
    debug_trace_mode: CpuAccessTraceMode,
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
        let sg_type_b_ram_extension = cartridge.uses_sg_type_b_ram_extension();
        let color_mode = mode4_color_mode_for_system(cartridge.system());
        Self {
            cartridge,
            boot_rom: None,
            mapper,
            sg_type_b_ram_extension,
            work_ram: [0; SMS_WORK_RAM_SIZE],
            cartridge_ram: [0; SMS_CARTRIDGE_RAM_SIZE],
            vdp: Vdp::new_with_video_standard_and_color_mode(video_standard, color_mode),
            apu: Apu::new_with_sample_rate(sample_rate),
            input: Input::new(),
            game_gear_serial: GameGearSerial::new(),
            memory_control: MEMORY_CONTROL_DEFAULT,
            rom_patches: Vec::new(),
            console_region,
            debug_trace_mode: CpuAccessTraceMode::None,
            debug_trace_events: RefCell::new(Vec::new()),
        }
    }

    pub fn mapper(&self) -> SegaMapper {
        self.mapper
    }

    pub(crate) fn set_boot_rom(&mut self, boot_rom: Option<Vec<u8>>) {
        self.boot_rom = boot_rom.map(Vec::into_boxed_slice);
    }

    pub fn has_boot_rom(&self) -> bool {
        self.boot_rom.is_some()
    }

    pub fn boot_rom_enabled(&self) -> bool {
        self.boot_rom.is_some() && self.memory_control & MEMORY_CONTROL_BOOT_ROM_DISABLE == 0
    }

    pub(crate) fn set_boot_rom_enabled(&mut self, enabled: bool) {
        if enabled {
            self.memory_control &= !MEMORY_CONTROL_BOOT_ROM_DISABLE;
        } else {
            self.memory_control |= MEMORY_CONTROL_BOOT_ROM_DISABLE;
        }
    }

    pub fn rom_offset_for_cpu_address(&self, addr: u16) -> Option<usize> {
        if self.boot_rom_read(addr).is_some() {
            return None;
        }
        if !self.cartridge_enabled_for_memory() {
            return None;
        }
        let offset = if let Some((page, page_offset, reverse_bits)) = self
            .mapper
            .rom_page_8k_mapping(addr, self.cartridge.rom_page_8k_count())
        {
            (!reverse_bits).then(|| usize::from(page) * ROM_PAGE_8K_SIZE + usize::from(page_offset))
        } else {
            match addr {
                SLOT0_START..=SLOT0_END => Some(
                    if self.mapper.kind() == Sega8MapperKind::Sega && addr < FIXED_BOOT_ROM_BYTES {
                        usize::from(addr)
                    } else {
                        usize::from(self.mapper.slot0_bank()) * usize::from(SLOT_SIZE)
                            + usize::from(addr % SLOT_SIZE)
                    },
                ),
                SLOT1_START..=SLOT1_END => Some(
                    usize::from(self.mapper.slot1_bank()) * usize::from(SLOT_SIZE)
                        + usize::from(addr - SLOT1_START),
                ),
                SLOT2_START..=SLOT2_END
                    if !self.mapper.slot2_cartridge_ram_enabled()
                        && self.mapper.codemasters_cartridge_ram_offset(addr).is_none() =>
                {
                    Some(
                        usize::from(self.mapper.slot2_bank()) * usize::from(SLOT_SIZE)
                            + usize::from(addr - SLOT2_START),
                    )
                }
                _ => None,
            }
        }?;
        Some(offset % self.cartridge.normalized_len())
    }

    pub fn rom_mapping_token(&self) -> u64 {
        let mapper = self.mapper;
        let [slot0, slot1, slot2] = mapper.slot_banks();
        u64::from(self.memory_control)
            | (u64::from(mapper.frame_control()) << 8)
            | (u64::from(slot0) << 16)
            | (u64::from(slot1) << 24)
            | (u64::from(slot2) << 32)
            | (u64::from(Self::mapper_kind_id(mapper.kind())) << 40)
    }

    pub fn work_ram(&self) -> &[u8] {
        &self.work_ram[..self.work_ram_size()]
    }

    pub fn cartridge_ram(&self) -> &[u8; SMS_CARTRIDGE_RAM_SIZE] {
        &self.cartridge_ram
    }

    pub fn cartridge_ram_visible(&self) -> &[u8] {
        &self.cartridge_ram[..self.cartridge.save_ram_kind().size()]
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

    pub fn memory_control(&self) -> u8 {
        self.memory_control
    }

    pub fn sync_game_gear_link_peer(&mut self, peer: &mut Self) {
        if self.cartridge.system() == Sega8System::GameGear
            && peer.cartridge.system() == Sega8System::GameGear
        {
            let clock_hz = self.video_standard().clock_hz_approx();
            self.game_gear_serial
                .exchange_with_peer(&mut peer.game_gear_serial, clock_hz);
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
        self.vdp
            .set_color_mode(mode4_color_mode_for_system(self.cartridge.system()));
        self.vdp.reset();
        self.apu.reset();
        self.input.reset();
        self.game_gear_serial.reset();
        self.memory_control = MEMORY_CONTROL_DEFAULT;
        self.rom_patches.clear();
        self.debug_trace_mode = CpuAccessTraceMode::None;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        self.vdp.step_cycles(cycles);
        self.apu.step_cycles(cycles);
        self.game_gear_serial
            .step_cycles(cycles, self.video_standard().clock_hz_approx());
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

    pub fn non_maskable_interrupt_pending(&self) -> bool {
        self.cartridge.system() == Sega8System::GameGear && self.game_gear_serial.rx_nmi_pending()
    }

    pub fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.cartridge.system() == Sega8System::GameGear
            && self.game_gear_serial.take_rx_nmi_pending()
    }

    pub fn begin_cpu_access_trace(&mut self) {
        self.debug_trace_mode = CpuAccessTraceMode::All;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub(crate) fn begin_cpu_write_trace(&mut self) {
        self.debug_trace_mode = CpuAccessTraceMode::Writes;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub fn drain_cpu_access_trace(&mut self) -> Vec<CpuAccessTraceEvent> {
        self.debug_trace_mode = CpuAccessTraceMode::None;
        std::mem::take(&mut *self.debug_trace_events.borrow_mut())
    }

    pub fn recycle_cpu_access_trace(&mut self, mut events: Vec<CpuAccessTraceEvent>) {
        events.clear();
        *self.debug_trace_events.borrow_mut() = events;
    }

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
        Ok(())
    }

    pub fn cpu_read(&self, addr: u16) -> u8 {
        let value = self.cpu_peek(addr);
        self.record_cpu_read(addr, value);
        value
    }

    pub fn cpu_peek(&self, addr: u16) -> u8 {
        let raw = self.cpu_read_raw(addr);
        self.apply_rom_patch(addr, raw)
    }

    fn cpu_read_raw(&self, addr: u16) -> u8 {
        if let Some(value) = self.boot_rom_read(addr) {
            return value;
        }
        match addr {
            SLOT0_START..=SLOT2_END if !self.cartridge_enabled_for_memory() => IO_OPEN_BUS_VALUE,
            SLOT0_START..=SLOT2_END
                if self
                    .mapper
                    .rom_page_8k_mapping(addr, self.cartridge.rom_page_8k_count())
                    .is_some() =>
            {
                let (page, offset, reverse_bits) = self
                    .mapper
                    .rom_page_8k_mapping(addr, self.cartridge.rom_page_8k_count())
                    .expect("8 KiB mapper page should be present");
                let value = self.cartridge.read_page_8k(page, offset);
                if reverse_bits {
                    value.reverse_bits()
                } else {
                    value
                }
            }
            SLOT0_START..=SLOT0_END => {
                if self.mapper.kind() == Sega8MapperKind::Sega && addr < FIXED_BOOT_ROM_BYTES {
                    self.cartridge.read_bank(0, addr)
                } else {
                    self.cartridge
                        .read_bank(self.mapper.slot0_bank(), addr % SLOT_SIZE)
                }
            }
            SLOT1_START..=SLOT1_END => self
                .cartridge
                .read_bank(self.mapper.slot1_bank(), addr.wrapping_sub(SLOT1_START)),
            SLOT2_START..=SLOT2_END => {
                if self.mapper.slot2_cartridge_ram_enabled() {
                    let offset = self.mapper.slot2_cartridge_ram_offset(addr);
                    self.cartridge_ram[offset]
                } else if let Some(offset) = self.mapper.codemasters_cartridge_ram_offset(addr) {
                    self.cartridge_ram[offset]
                } else {
                    self.cartridge
                        .read_bank(self.mapper.slot2_bank(), addr.wrapping_sub(SLOT2_START))
                }
            }
            WORK_RAM_START..=WORK_RAM_END if self.work_ram_enabled_for_memory() => {
                self.work_ram[self.work_ram_offset(addr)]
            }
            WORK_RAM_START..=WORK_RAM_END => IO_OPEN_BUS_VALUE,
        }
    }

    fn boot_rom_read(&self, addr: u16) -> Option<u8> {
        if !self.boot_rom_enabled() {
            return None;
        }
        self.boot_rom.as_deref()?.get(usize::from(addr)).copied()
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
        self.boot_rom_read(addr).is_none()
            && self.cartridge_enabled_for_memory()
            && (matches!(addr, SLOT0_START..=SLOT1_END)
                || matches!(addr, SLOT2_START..=SLOT2_END)
                    && !self.mapper.slot2_cartridge_ram_enabled()
                    && self.mapper.codemasters_cartridge_ram_offset(addr).is_none())
    }

    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        let old_value = self.cpu_read_raw(addr);
        match addr {
            SLOT2_START..=SLOT2_END
                if self.cartridge_enabled_for_memory()
                    && self.mapper.codemasters_cartridge_ram_offset(addr).is_some() =>
            {
                let offset = self
                    .mapper
                    .codemasters_cartridge_ram_offset(addr)
                    .expect("Codemasters RAM offset should be present");
                self.cartridge_ram[offset] = val;
            }
            SLOT0_START..=SLOT2_END
                if self.cartridge_enabled_for_memory()
                    && self.mapper.kind() == Sega8MapperKind::Codemasters =>
            {
                self.mapper.write_codemasters_register(addr, val);
            }
            SLOT0_START..=SLOT0_END
                if self.cartridge_enabled_for_memory()
                    && matches!(
                        self.mapper.kind(),
                        Sega8MapperKind::Msx | Sega8MapperKind::Nemesis
                    ) =>
            {
                self.mapper.write_msx_register(addr, val);
            }
            SLOT1_START..=SLOT2_END
                if self.cartridge_enabled_for_memory()
                    && self.mapper.kind() == Sega8MapperKind::Janggun =>
            {
                self.mapper.write_janggun_register(addr, val);
            }
            SLOT2_START..=SLOT2_END
                if self.cartridge_enabled_for_memory()
                    && self.mapper.kind() == Sega8MapperKind::Korean =>
            {
                self.mapper.write_korean_register(addr, val);
            }
            SLOT2_START..=SLOT2_END
                if self.cartridge_enabled_for_memory()
                    && self.mapper.slot2_cartridge_ram_enabled() =>
            {
                let offset = self.mapper.slot2_cartridge_ram_offset(addr);
                self.cartridge_ram[offset] = val;
            }
            WORK_RAM_START..=WORK_RAM_END => {
                if self.work_ram_enabled_for_memory() {
                    let offset = self.work_ram_offset(addr);
                    self.work_ram[offset] = val;
                }
                if self.cartridge.system() != Sega8System::Sg1000
                    && self.cartridge_enabled_for_memory()
                {
                    if self.mapper.kind() == Sega8MapperKind::Janggun {
                        self.mapper.write_janggun_register(addr, val);
                    } else {
                        self.mapper.write_sega_register(addr, val);
                    }
                }
            }
            _ => {}
        }
        self.record_cpu_write(addr, old_value, val);
    }

    fn cartridge_enabled_for_memory(&self) -> bool {
        self.cartridge.system() != Sega8System::MasterSystem
            || self.memory_control & MEMORY_CONTROL_CARTRIDGE_DISABLE == 0
    }

    fn mapper_kind_id(kind: Sega8MapperKind) -> u8 {
        match kind {
            Sega8MapperKind::Sega => 0,
            Sega8MapperKind::Codemasters => 1,
            Sega8MapperKind::Korean => 2,
            Sega8MapperKind::Msx => 3,
            Sega8MapperKind::Nemesis => 4,
            Sega8MapperKind::Janggun => 5,
        }
    }

    fn work_ram_enabled_for_memory(&self) -> bool {
        self.cartridge.system() != Sega8System::MasterSystem
            || self.memory_control & MEMORY_CONTROL_WORK_RAM_DISABLE == 0
    }

    fn io_enabled_for_memory(&self) -> bool {
        self.cartridge.system() != Sega8System::MasterSystem
            || self.memory_control & MEMORY_CONTROL_IO_DISABLE == 0
    }

    fn work_ram_size(&self) -> usize {
        match self.cartridge.system() {
            Sega8System::Sg1000 if !self.sg_type_b_ram_extension => SG_WORK_RAM_SIZE,
            Sega8System::Sg1000 => SMS_WORK_RAM_SIZE,
            Sega8System::MasterSystem | Sega8System::GameGear => SMS_WORK_RAM_SIZE,
        }
    }

    fn work_ram_offset(&self, addr: u16) -> usize {
        usize::from(addr & self.work_ram_mask())
    }

    fn work_ram_mask(&self) -> u16 {
        match self.cartridge.system() {
            Sega8System::Sg1000 if !self.sg_type_b_ram_extension => SG_WORK_RAM_MASK,
            Sega8System::Sg1000 => WORK_RAM_MASK,
            Sega8System::MasterSystem | Sega8System::GameGear => WORK_RAM_MASK,
        }
    }

    pub fn io_read(&mut self, port: u8) -> u8 {
        let value = if self.io_enabled_for_memory() {
            match port {
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
            }
        } else {
            IO_OPEN_BUS_VALUE
        };
        self.record_io_read(port, value);
        value
    }

    pub fn io_write(&mut self, port: u8, val: u8) {
        if self.write_memory_control_port(port, val) {
            self.record_io_write(port, val);
            return;
        }

        if !self.io_enabled_for_memory() {
            self.record_io_write(port, val);
            return;
        }

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

    fn write_memory_control_port(&mut self, port: u8, val: u8) -> bool {
        let decoded = match self.cartridge.system() {
            Sega8System::MasterSystem => is_memory_control_mirror(port),
            Sega8System::GameGear => self.boot_rom.is_some() && port == IO_PORT_MEMORY_CONTROL,
            Sega8System::Sg1000 => false,
        };
        if !decoded {
            return false;
        }

        self.memory_control = val;
        true
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
        if self.debug_trace_mode == CpuAccessTraceMode::All {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: u32::from(addr),
                    value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }

    fn record_cpu_write(&self, addr: u16, old_value: u8, new_value: u8) {
        if self.debug_trace_mode != CpuAccessTraceMode::None {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Write {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: u32::from(addr),
                    old_value: u32::from(old_value),
                    written_value: u32::from(new_value),
                    new_value: u32::from(new_value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }

    fn record_io_read(&self, port: u8, value: u8) {
        if self.debug_trace_mode == CpuAccessTraceMode::All {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Read {
                    at: None,
                    space: TraceWriteKind::Io,
                    addr: u32::from(port),
                    value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }

    fn record_io_write(&self, port: u8, value: u8) {
        if self.debug_trace_mode != CpuAccessTraceMode::None {
            self.debug_trace_events
                .borrow_mut()
                .push(CpuAccessTraceEvent::Write {
                    at: None,
                    space: TraceWriteKind::Io,
                    addr: u32::from(port),
                    old_value: u32::from(value),
                    written_value: u32::from(value),
                    new_value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }
}

fn is_vdp_data_mirror(port: u8) -> bool {
    port & IO_PORT_VDP_MIRROR_MASK == IO_PORT_VDP_DATA_MIRROR_VALUE
}

fn is_vdp_control_mirror(port: u8) -> bool {
    port & IO_PORT_VDP_MIRROR_MASK == IO_PORT_VDP_CONTROL_MIRROR_VALUE
}

fn mode4_color_mode_for_system(system: Sega8System) -> Mode4ColorMode {
    match system {
        Sega8System::GameGear => Mode4ColorMode::GameGear,
        Sega8System::MasterSystem | Sega8System::Sg1000 => Mode4ColorMode::Sms,
    }
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

fn is_memory_control_mirror(port: u8) -> bool {
    port == IO_PORT_MEMORY_CONTROL || port < 0x40 && port & 1 == 0
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

#[cfg(test)]
mod tests;
