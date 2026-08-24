use anyhow::bail;
use bitflags::bitflags;
use zeff_emu_common::save_state::{StateReader, StateWriter};

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct PadButtons: u8 {
        const I = 1 << 0;
        const II = 1 << 1;
        const SELECT = 1 << 2;
        const RUN = 1 << 3;
        const UP = 1 << 4;
        const RIGHT = 1 << 5;
        const DOWN = 1 << 6;
        const LEFT = 1 << 7;
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct SixButtonExtraButtons: u8 {
        const III = 1 << 0;
        const IV = 1 << 1;
        const V = 1 << 2;
        const VI = 1 << 3;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceControllerMode {
    #[default]
    Automatic,
    TwoButton,
    SixButton,
    Multitap,
    Mouse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceMemoryBaseMode {
    #[default]
    Automatic,
    Enabled,
    Disabled,
}

pub const MEMORY_BASE128_RAM_LEN: usize = 128 * 1024;
pub const MAX_CONTROLLER_STATE_SECTION_BYTES: usize = MEMORY_BASE128_RAM_LEN + 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryBase128Phase {
    #[default]
    Idle,
    IdentifyFirst,
    IdentifySecond,
    Request,
    Address,
    Length,
    Read,
    ReadTrail,
    Write,
    WriteTrail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryBase128DebugSnapshot {
    pub connected: bool,
    pub active: bool,
    pub phase: MemoryBase128Phase,
    pub activation_shift: u8,
    pub bit_index: u8,
    pub read_command: bool,
    pub address: u32,
    pub remaining_bits: u32,
    pub output_nibble: u8,
    pub dirty: bool,
}

#[derive(Debug)]
pub struct MemoryBase128 {
    ram: Box<[u8]>,
    connected: bool,
    activation_shift: u8,
    active: bool,
    phase: MemoryBase128Phase,
    bit_index: u8,
    read_command: bool,
    address: u32,
    remaining_bits: u32,
    output_nibble: u8,
    dirty: bool,
}

impl Default for MemoryBase128 {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBase128 {
    pub fn new() -> Self {
        Self {
            ram: vec![0; MEMORY_BASE128_RAM_LEN].into_boxed_slice(),
            connected: false,
            activation_shift: 0xFF,
            active: false,
            phase: MemoryBase128Phase::Idle,
            bit_index: 0,
            read_command: false,
            address: 0,
            remaining_bits: 0,
            output_nibble: 0,
            dirty: false,
        }
    }

    pub const fn is_connected(&self) -> bool {
        self.connected
    }

    pub const fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_connected(&mut self, connected: bool) {
        if self.connected != connected {
            self.connected = connected;
            self.reset_protocol();
        }
    }

    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn load_ram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if bytes.len() != MEMORY_BASE128_RAM_LEN {
            bail!(
                "invalid Memory Base 128 image size: {} (expected {MEMORY_BASE128_RAM_LEN})",
                bytes.len()
            );
        }
        self.ram.copy_from_slice(bytes);
        self.dirty = false;
        Ok(())
    }

    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub const fn debug_snapshot(&self) -> MemoryBase128DebugSnapshot {
        MemoryBase128DebugSnapshot {
            connected: self.connected,
            active: self.active,
            phase: self.phase,
            activation_shift: self.activation_shift,
            bit_index: self.bit_index,
            read_command: self.read_command,
            address: self.address,
            remaining_bits: self.remaining_bits,
            output_nibble: self.output_nibble,
            dirty: self.dirty,
        }
    }

    fn reset_protocol(&mut self) {
        self.activation_shift = 0xFF;
        self.active = false;
        self.phase = MemoryBase128Phase::Idle;
        self.bit_index = 0;
        self.read_command = false;
        self.address = 0;
        self.remaining_bits = 0;
        self.output_nibble = 0;
    }

    fn write_lines(&mut self, previous_clear_high: bool, select_high: bool, clear_high: bool) {
        if !self.connected || previous_clear_high || !clear_high {
            return;
        }
        if self.active {
            self.send_bit(select_high);
            return;
        }
        self.activation_shift = (self.activation_shift >> 1) | (u8::from(select_high) << 7);
        if self.activation_shift == 0xA8 {
            self.active = true;
            self.phase = MemoryBase128Phase::IdentifyFirst;
        }
    }

    fn send_bit(&mut self, input: bool) {
        match self.phase {
            MemoryBase128Phase::Idle => {}
            MemoryBase128Phase::IdentifyFirst => {
                self.output_nibble = u8::from(input) << 2;
                self.phase = MemoryBase128Phase::IdentifySecond;
            }
            MemoryBase128Phase::IdentifySecond => {
                self.output_nibble = u8::from(input) << 2;
                self.phase = MemoryBase128Phase::Request;
            }
            MemoryBase128Phase::Request => {
                self.read_command = input;
                self.address = 0;
                self.bit_index = 0;
                self.output_nibble = 0;
                self.phase = MemoryBase128Phase::Address;
            }
            MemoryBase128Phase::Address => {
                self.address |= u32::from(input) << (u32::from(self.bit_index) + 7);
                self.output_nibble = 0;
                self.bit_index += 1;
                if self.bit_index == 10 {
                    self.bit_index = 0;
                    self.remaining_bits = 0;
                    self.phase = MemoryBase128Phase::Length;
                }
            }
            MemoryBase128Phase::Length => {
                self.remaining_bits |= u32::from(input) << self.bit_index;
                self.output_nibble = 0;
                self.bit_index += 1;
                if self.bit_index == 20 {
                    self.bit_index = 0;
                    self.phase = match (self.read_command, self.remaining_bits == 0) {
                        (true, false) => MemoryBase128Phase::Read,
                        (true, true) => MemoryBase128Phase::ReadTrail,
                        (false, false) => MemoryBase128Phase::Write,
                        (false, true) => MemoryBase128Phase::WriteTrail,
                    };
                }
            }
            MemoryBase128Phase::Read => {
                let address = self.address as usize & (MEMORY_BASE128_RAM_LEN - 1);
                self.output_nibble = (self.ram[address] >> self.bit_index) & 1;
                self.advance_transfer_bit(MemoryBase128Phase::ReadTrail);
            }
            MemoryBase128Phase::Write => {
                let address = self.address as usize & (MEMORY_BASE128_RAM_LEN - 1);
                let mask = 1 << self.bit_index;
                self.ram[address] =
                    (self.ram[address] & !mask) | (u8::from(input) << self.bit_index);
                self.dirty = true;
                self.output_nibble = 0;
                self.advance_transfer_bit(MemoryBase128Phase::WriteTrail);
            }
            MemoryBase128Phase::WriteTrail => {
                self.bit_index += 1;
                if self.bit_index == 2 {
                    self.output_nibble = 0;
                } else if self.bit_index == 3 {
                    self.bit_index = 0;
                    self.phase = MemoryBase128Phase::ReadTrail;
                }
            }
            MemoryBase128Phase::ReadTrail => {
                self.bit_index += 1;
                if self.bit_index == 2 {
                    self.output_nibble = 0;
                } else if self.bit_index == 4 {
                    self.reset_protocol();
                }
            }
        }
    }

    fn advance_transfer_bit(&mut self, trailing_phase: MemoryBase128Phase) {
        self.bit_index += 1;
        self.remaining_bits -= 1;
        if self.remaining_bits == 0 {
            self.bit_index = 0;
            self.phase = trailing_phase;
        } else if self.bit_index == 8 {
            self.bit_index = 0;
            self.address = self.address.wrapping_add(1);
        }
    }

    fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.connected);
        writer.write_u8(self.activation_shift);
        writer.write_bool(self.active);
        writer.write_u8(memory_base_phase_to_tag(self.phase));
        writer.write_u8(self.bit_index);
        writer.write_bool(self.read_command);
        writer.write_u32(self.address);
        writer.write_u32(self.remaining_bits);
        writer.write_u8(self.output_nibble);
        writer.write_bool(self.dirty);
        writer.write_vec(&self.ram);
    }

    fn read_state(reader: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let connected = reader.read_bool()?;
        let activation_shift = reader.read_u8()?;
        let active = reader.read_bool()?;
        let phase = tag_to_memory_base_phase(reader.read_u8()?)?;
        let bit_index = reader.read_u8()?;
        let read_command = reader.read_bool()?;
        let address = reader.read_u32()?;
        let remaining_bits = reader.read_u32()?;
        let output_nibble = reader.read_u8()?;
        let dirty = reader.read_bool()?;
        let ram = reader.read_vec(MEMORY_BASE128_RAM_LEN)?;

        if ram.len() != MEMORY_BASE128_RAM_LEN {
            bail!(
                "invalid Memory Base 128 RAM length in save-state: {}",
                ram.len()
            );
        }
        if active != (phase != MemoryBase128Phase::Idle) {
            bail!("inconsistent Memory Base 128 active state in save-state");
        }
        if active && !connected {
            bail!("disconnected Memory Base 128 is active in save-state");
        }
        if remaining_bits > 0x000F_FFFF {
            bail!("invalid Memory Base 128 transfer length in save-state: {remaining_bits}");
        }
        if output_nibble > 0x0F {
            bail!("invalid Memory Base 128 output nibble in save-state: {output_nibble}");
        }
        let valid_bit_index = match phase {
            MemoryBase128Phase::Idle
            | MemoryBase128Phase::IdentifyFirst
            | MemoryBase128Phase::IdentifySecond
            | MemoryBase128Phase::Request => bit_index == 0,
            MemoryBase128Phase::Address => bit_index < 10,
            MemoryBase128Phase::Length => bit_index < 20,
            MemoryBase128Phase::Read | MemoryBase128Phase::Write => {
                bit_index < 8 && remaining_bits != 0
            }
            MemoryBase128Phase::ReadTrail => bit_index < 4 && remaining_bits == 0,
            MemoryBase128Phase::WriteTrail => bit_index < 3 && remaining_bits == 0,
        };
        if !valid_bit_index {
            bail!("invalid Memory Base 128 bit index for {phase:?} in save-state: {bit_index}");
        }

        Ok(Self {
            ram: ram.into_boxed_slice(),
            connected,
            activation_shift,
            active,
            phase,
            bit_index,
            read_command,
            address,
            remaining_bits,
            output_nibble,
            dirty,
        })
    }
}

const fn memory_base_phase_to_tag(phase: MemoryBase128Phase) -> u8 {
    match phase {
        MemoryBase128Phase::Idle => 0,
        MemoryBase128Phase::IdentifyFirst => 1,
        MemoryBase128Phase::IdentifySecond => 2,
        MemoryBase128Phase::Request => 3,
        MemoryBase128Phase::Address => 4,
        MemoryBase128Phase::Length => 5,
        MemoryBase128Phase::Read => 6,
        MemoryBase128Phase::ReadTrail => 7,
        MemoryBase128Phase::Write => 8,
        MemoryBase128Phase::WriteTrail => 9,
    }
}

fn tag_to_memory_base_phase(tag: u8) -> anyhow::Result<MemoryBase128Phase> {
    Ok(match tag {
        0 => MemoryBase128Phase::Idle,
        1 => MemoryBase128Phase::IdentifyFirst,
        2 => MemoryBase128Phase::IdentifySecond,
        3 => MemoryBase128Phase::Request,
        4 => MemoryBase128Phase::Address,
        5 => MemoryBase128Phase::Length,
        6 => MemoryBase128Phase::Read,
        7 => MemoryBase128Phase::ReadTrail,
        8 => MemoryBase128Phase::Write,
        9 => MemoryBase128Phase::WriteTrail,
        _ => bail!("invalid Memory Base 128 phase tag in save-state: {tag}"),
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TwoButtonPad {
    buttons: PadButtons,
}

impl TwoButtonPad {
    #[inline]
    pub const fn new() -> Self {
        Self {
            buttons: PadButtons::empty(),
        }
    }

    #[inline]
    pub const fn buttons(self) -> PadButtons {
        self.buttons
    }

    #[inline]
    pub fn set_buttons(&mut self, buttons: PadButtons) {
        self.buttons = buttons;
    }

    #[inline]
    pub fn set_button(&mut self, button: PadButtons, pressed: bool) {
        self.buttons.set(button, pressed);
    }

    #[inline]
    fn read_nibble(self, select_high: bool) -> u8 {
        let pressed = if select_high {
            self.buttons.bits() >> 4
        } else {
            self.buttons.bits()
        };
        !pressed & 0x0F
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SixButtonPhase {
    #[default]
    Standard,
    Extended,
}

pub const DETERMINISTIC_SIX_BUTTON_RESET_PHASE: SixButtonPhase = SixButtonPhase::Standard;

// Hardware measurements are approximate: about 550 us without the expected SEL
// transition, or about 600 us without another CLR pulse.
pub const PROVISIONAL_MOUSE_SELECT_TIMEOUT_MASTER_TICKS: u64 = 11_813;
pub const PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS: u64 = 12_886;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseScanPhase {
    #[default]
    XHigh,
    XLow,
    YHigh,
    YLow,
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceMouseDebugSnapshot {
    pub buttons: PadButtons,
    pub pending_x: i32,
    pub pending_y: i32,
    pub latched_x: u8,
    pub latched_y: u8,
    pub phase: MouseScanPhase,
    pub scan_active: bool,
    pub select_elapsed: u64,
    pub clear_elapsed: u64,
}

impl MouseScanPhase {
    const fn next(self) -> Self {
        match self {
            Self::XHigh => Self::XLow,
            Self::XLow => Self::YHigh,
            Self::YHigh => Self::YLow,
            Self::YLow | Self::Exhausted => Self::Exhausted,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PceMouse {
    buttons: PadButtons,
    pending_x: i32,
    pending_y: i32,
    latched_x: u8,
    latched_y: u8,
    phase: MouseScanPhase,
    scan_active: bool,
    select_elapsed: u64,
    clear_elapsed: u64,
}

impl PceMouse {
    pub const fn new() -> Self {
        Self {
            buttons: PadButtons::empty(),
            pending_x: 0,
            pending_y: 0,
            latched_x: 0,
            latched_y: 0,
            phase: MouseScanPhase::XHigh,
            scan_active: false,
            select_elapsed: 0,
            clear_elapsed: 0,
        }
    }

    pub fn set_buttons(&mut self, buttons: PadButtons) {
        self.buttons = buttons;
    }

    pub fn set_button(&mut self, button: PadButtons, pressed: bool) {
        self.buttons.set(button, pressed);
    }

    pub fn accumulate_motion(&mut self, delta_x: i16, delta_y: i16) {
        self.pending_x = self.pending_x.saturating_add(i32::from(delta_x));
        self.pending_y = self.pending_y.saturating_add(i32::from(delta_y));
    }

    #[inline]
    pub const fn debug_snapshot(self) -> PceMouseDebugSnapshot {
        PceMouseDebugSnapshot {
            buttons: self.buttons,
            pending_x: self.pending_x,
            pending_y: self.pending_y,
            latched_x: self.latched_x,
            latched_y: self.latched_y,
            phase: self.phase,
            scan_active: self.scan_active,
            select_elapsed: self.select_elapsed,
            clear_elapsed: self.clear_elapsed,
        }
    }

    fn reset(&mut self) {
        self.pending_x = 0;
        self.pending_y = 0;
        self.finish_scan();
    }

    fn finish_scan(&mut self) {
        self.latched_x = 0;
        self.latched_y = 0;
        self.phase = MouseScanPhase::XHigh;
        self.scan_active = false;
        self.select_elapsed = 0;
        self.clear_elapsed = 0;
    }

    fn latch_axis(value: &mut i32) -> u8 {
        let latched = (*value).clamp(-127, 127) as i8;
        *value -= i32::from(latched);
        latched as u8
    }

    fn start_scan(&mut self) {
        self.latched_x = Self::latch_axis(&mut self.pending_x);
        self.latched_y = Self::latch_axis(&mut self.pending_y);
        self.phase = MouseScanPhase::XHigh;
        self.scan_active = true;
        self.select_elapsed = 0;
        self.clear_elapsed = 0;
    }

    fn write_lines(
        &mut self,
        previous_select_high: bool,
        previous_clear_high: bool,
        select_high: bool,
        clear_high: bool,
    ) {
        if previous_select_high != select_high {
            self.select_elapsed = 0;
        }
        if !previous_clear_high && clear_high {
            if self.scan_active {
                self.phase = self.phase.next();
                self.clear_elapsed = 0;
            } else {
                self.start_scan();
            }
        }
    }

    fn advance_master_ticks(&mut self, master_ticks: u64) {
        if !self.scan_active {
            return;
        }
        self.select_elapsed = self.select_elapsed.saturating_add(master_ticks);
        self.clear_elapsed = self.clear_elapsed.saturating_add(master_ticks);
        if self.select_elapsed >= PROVISIONAL_MOUSE_SELECT_TIMEOUT_MASTER_TICKS
            || self.clear_elapsed >= PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS
        {
            self.finish_scan();
        }
    }

    fn read_nibble(self, select_high: bool, clear_high: bool) -> u8 {
        if clear_high {
            return 0;
        }
        if !select_high {
            return !self.buttons.bits() & 0x0F;
        }
        match self.phase {
            MouseScanPhase::XHigh => self.latched_x >> 4,
            MouseScanPhase::XLow => self.latched_x & 0x0F,
            MouseScanPhase::YHigh => self.latched_y >> 4,
            MouseScanPhase::YLow => self.latched_y & 0x0F,
            MouseScanPhase::Exhausted => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SixButtonPad {
    standard: TwoButtonPad,
    extra_buttons: SixButtonExtraButtons,
    phase: SixButtonPhase,
}

impl SixButtonPad {
    #[inline]
    pub const fn new() -> Self {
        Self::with_phase(DETERMINISTIC_SIX_BUTTON_RESET_PHASE)
    }

    #[inline]
    pub const fn with_phase(phase: SixButtonPhase) -> Self {
        Self {
            standard: TwoButtonPad::new(),
            extra_buttons: SixButtonExtraButtons::empty(),
            phase,
        }
    }

    #[inline]
    pub const fn standard_pad(&self) -> &TwoButtonPad {
        &self.standard
    }

    #[inline]
    pub fn standard_pad_mut(&mut self) -> &mut TwoButtonPad {
        &mut self.standard
    }

    #[inline]
    pub const fn extra_buttons(self) -> SixButtonExtraButtons {
        self.extra_buttons
    }

    #[inline]
    pub fn set_extra_buttons(&mut self, buttons: SixButtonExtraButtons) {
        self.extra_buttons = buttons;
    }

    #[inline]
    pub fn set_extra_button(&mut self, button: SixButtonExtraButtons, pressed: bool) {
        self.extra_buttons.set(button, pressed);
    }

    #[inline]
    pub const fn phase(self) -> SixButtonPhase {
        self.phase
    }

    #[inline]
    fn toggle_phase(&mut self) {
        self.phase = match self.phase {
            SixButtonPhase::Standard => SixButtonPhase::Extended,
            SixButtonPhase::Extended => SixButtonPhase::Standard,
        };
    }

    #[inline]
    fn reset(&mut self) {
        self.phase = DETERMINISTIC_SIX_BUTTON_RESET_PHASE;
    }

    #[inline]
    fn read_nibble(self, select_high: bool) -> u8 {
        match (self.phase, select_high) {
            (SixButtonPhase::Standard, _) => self.standard.read_nibble(select_high),
            (SixButtonPhase::Extended, true) => 0,
            (SixButtonPhase::Extended, false) => !self.extra_buttons.bits() & 0x0F,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MultitapDevice {
    #[default]
    Disconnected,
    TwoButton(TwoButtonPad),
    SixButton(SixButtonPad),
}

impl MultitapDevice {
    #[inline]
    fn reset(&mut self) {
        if let Self::SixButton(pad) = self {
            pad.reset();
        }
    }

    #[inline]
    fn write_lines(&mut self, previous_clear_high: bool, clear_high: bool) {
        if !previous_clear_high
            && clear_high
            && let Self::SixButton(pad) = self
        {
            pad.toggle_phase();
        }
    }

    #[inline]
    fn read_nibble(self, select_high: bool, clear_high: bool) -> u8 {
        match self {
            Self::Disconnected => 0x0F,
            Self::TwoButton(_) | Self::SixButton(_) if clear_high => 0,
            Self::TwoButton(pad) => pad.read_nibble(select_high),
            Self::SixButton(pad) => pad.read_nibble(select_high),
        }
    }
}

pub const MULTITAP_EXHAUSTED_NIBBLE: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultitapPort {
    One,
    Two,
    Three,
    Four,
    Five,
}

impl MultitapPort {
    #[inline]
    const fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
            Self::Three => 2,
            Self::Four => 3,
            Self::Five => 4,
        }
    }

    #[inline]
    const fn next(self) -> Option<Self> {
        match self {
            Self::One => Some(Self::Two),
            Self::Two => Some(Self::Three),
            Self::Three => Some(Self::Four),
            Self::Four => Some(Self::Five),
            Self::Five => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FivePortMultitap {
    ports: [MultitapDevice; 5],
    active_port: Option<MultitapPort>,
}

impl Default for FivePortMultitap {
    fn default() -> Self {
        Self::new([MultitapDevice::Disconnected; 5])
    }
}

impl FivePortMultitap {
    pub const fn new(ports: [MultitapDevice; 5]) -> Self {
        Self {
            ports,
            active_port: None,
        }
    }

    #[inline]
    pub const fn active_port(&self) -> Option<MultitapPort> {
        self.active_port
    }

    #[inline]
    pub const fn port(&self, port: MultitapPort) -> &MultitapDevice {
        &self.ports[port.index()]
    }

    #[inline]
    pub fn port_mut(&mut self, port: MultitapPort) -> &mut MultitapDevice {
        &mut self.ports[port.index()]
    }

    fn reset(&mut self) {
        self.active_port = None;
        for device in &mut self.ports {
            device.reset();
        }
    }

    fn write_lines(
        &mut self,
        previous_select_high: bool,
        previous_clear_high: bool,
        select_high: bool,
        clear_high: bool,
    ) {
        let previous_active_port = self.active_port;
        if !previous_clear_high && clear_high && select_high {
            self.active_port = Some(MultitapPort::One);
        } else if !previous_select_high && select_high && !clear_high {
            self.active_port = self.active_port.and_then(MultitapPort::next);
        }

        for (index, device) in self.ports.iter_mut().enumerate() {
            let (_, previous_device_clear) = Self::routed_lines(
                previous_active_port,
                previous_select_high,
                previous_clear_high,
                index,
            );
            let (_, device_clear) =
                Self::routed_lines(self.active_port, select_high, clear_high, index);
            device.write_lines(previous_device_clear, device_clear);
        }
    }

    #[inline]
    fn read_nibble(self, select_high: bool, clear_high: bool) -> u8 {
        self.active_port
            .map(|port| self.ports[port.index()].read_nibble(select_high, clear_high))
            .unwrap_or(MULTITAP_EXHAUSTED_NIBBLE)
    }

    #[inline]
    fn routed_lines(
        active_port: Option<MultitapPort>,
        select_high: bool,
        clear_high: bool,
        port_index: usize,
    ) -> (bool, bool) {
        if !clear_high && active_port.is_some_and(|port| port.index() == port_index) {
            (select_high, false)
        } else {
            (true, true)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerDevice {
    #[default]
    Disconnected,
    TwoButton(TwoButtonPad),
    SixButton(SixButtonPad),
    Multitap(FivePortMultitap),
    Mouse(PceMouse),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultitapDeviceDebugSnapshot {
    Disconnected,
    TwoButton {
        buttons: PadButtons,
    },
    SixButton {
        buttons: PadButtons,
        extra_buttons: SixButtonExtraButtons,
        phase: SixButtonPhase,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerDeviceDebugSnapshot {
    Disconnected,
    TwoButton {
        buttons: PadButtons,
    },
    SixButton {
        buttons: PadButtons,
        extra_buttons: SixButtonExtraButtons,
        phase: SixButtonPhase,
    },
    Multitap {
        active_port: Option<MultitapPort>,
        ports: [MultitapDeviceDebugSnapshot; 5],
    },
    Mouse(PceMouseDebugSnapshot),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerPortDebugSnapshot {
    pub device: ControllerDeviceDebugSnapshot,
    pub select_high: bool,
    pub clear_high: bool,
    pub input_nibble: u8,
    pub memory_base: MemoryBase128DebugSnapshot,
}

#[derive(Debug)]
pub struct ControllerPort {
    device: ControllerDevice,
    select_high: bool,
    clear_high: bool,
    memory_base: MemoryBase128,
}

impl Default for ControllerPort {
    fn default() -> Self {
        Self::new(ControllerDevice::Disconnected)
    }
}

impl ControllerPort {
    pub fn new(device: ControllerDevice) -> Self {
        Self {
            device,
            select_high: true,
            clear_high: true,
            memory_base: MemoryBase128::new(),
        }
    }

    #[inline]
    pub fn two_button() -> Self {
        Self::new(ControllerDevice::TwoButton(TwoButtonPad::new()))
    }

    #[inline]
    pub fn six_button() -> Self {
        Self::new(ControllerDevice::SixButton(SixButtonPad::new()))
    }

    #[inline]
    pub fn multitap(multitap: FivePortMultitap) -> Self {
        Self::new(ControllerDevice::Multitap(multitap))
    }

    #[inline]
    pub fn mouse() -> Self {
        Self::new(ControllerDevice::Mouse(PceMouse::new()))
    }

    #[inline]
    pub fn reset(&mut self) {
        match &mut self.device {
            ControllerDevice::SixButton(pad) => pad.reset(),
            ControllerDevice::Multitap(multitap) => multitap.reset(),
            ControllerDevice::Mouse(mouse) => mouse.reset(),
            _ => {}
        }
        self.select_high = true;
        self.clear_high = true;
        self.memory_base.reset_protocol();
    }

    pub fn debug_snapshot(&self) -> ControllerPortDebugSnapshot {
        ControllerPortDebugSnapshot {
            device: match self.device {
                ControllerDevice::Disconnected => ControllerDeviceDebugSnapshot::Disconnected,
                ControllerDevice::TwoButton(pad) => ControllerDeviceDebugSnapshot::TwoButton {
                    buttons: pad.buttons(),
                },
                ControllerDevice::SixButton(pad) => ControllerDeviceDebugSnapshot::SixButton {
                    buttons: pad.standard_pad().buttons(),
                    extra_buttons: pad.extra_buttons(),
                    phase: pad.phase(),
                },
                ControllerDevice::Multitap(multitap) => ControllerDeviceDebugSnapshot::Multitap {
                    active_port: multitap.active_port(),
                    ports: [
                        MultitapPort::One,
                        MultitapPort::Two,
                        MultitapPort::Three,
                        MultitapPort::Four,
                        MultitapPort::Five,
                    ]
                    .map(|port| multitap_device_debug_snapshot(*multitap.port(port))),
                },
                ControllerDevice::Mouse(mouse) => {
                    ControllerDeviceDebugSnapshot::Mouse(mouse.debug_snapshot())
                }
            },
            select_high: self.select_high,
            clear_high: self.clear_high,
            input_nibble: self.read_nibble(),
            memory_base: self.memory_base.debug_snapshot(),
        }
    }

    #[inline]
    pub const fn device(&self) -> ControllerDevice {
        self.device
    }

    #[inline]
    pub fn set_device(&mut self, device: ControllerDevice) {
        self.device = device;
    }

    #[inline]
    pub fn two_button_pad_mut(&mut self) -> Option<&mut TwoButtonPad> {
        match &mut self.device {
            ControllerDevice::TwoButton(pad) => Some(pad),
            _ => None,
        }
    }

    #[inline]
    pub fn six_button_pad_mut(&mut self) -> Option<&mut SixButtonPad> {
        match &mut self.device {
            ControllerDevice::SixButton(pad) => Some(pad),
            _ => None,
        }
    }

    #[inline]
    pub fn multitap_mut(&mut self) -> Option<&mut FivePortMultitap> {
        match &mut self.device {
            ControllerDevice::Multitap(multitap) => Some(multitap),
            _ => None,
        }
    }

    #[inline]
    pub fn mouse_mut(&mut self) -> Option<&mut PceMouse> {
        match &mut self.device {
            ControllerDevice::Mouse(mouse) => Some(mouse),
            _ => None,
        }
    }

    #[inline]
    pub const fn memory_base128(&self) -> &MemoryBase128 {
        &self.memory_base
    }

    #[inline]
    pub fn memory_base128_mut(&mut self) -> &mut MemoryBase128 {
        &mut self.memory_base
    }

    #[inline]
    pub fn set_memory_base128_connected(&mut self, connected: bool) {
        self.memory_base.set_connected(connected);
    }

    #[inline]
    pub fn advance_master_ticks(&mut self, master_ticks: u64) {
        if let ControllerDevice::Mouse(mouse) = &mut self.device {
            mouse.advance_master_ticks(master_ticks);
        }
    }

    #[inline]
    pub fn write_lines(&mut self, select_high: bool, clear_high: bool) {
        self.memory_base
            .write_lines(self.clear_high, select_high, clear_high);
        match &mut self.device {
            ControllerDevice::SixButton(pad) if !self.clear_high && clear_high => {
                pad.toggle_phase();
            }
            ControllerDevice::Multitap(multitap) => {
                multitap.write_lines(self.select_high, self.clear_high, select_high, clear_high)
            }
            ControllerDevice::Mouse(mouse) => {
                mouse.write_lines(self.select_high, self.clear_high, select_high, clear_high)
            }
            _ => {}
        }
        self.select_high = select_high;
        self.clear_high = clear_high;
    }

    #[inline]
    pub const fn select_high(&self) -> bool {
        self.select_high
    }

    #[inline]
    pub const fn clear_high(&self) -> bool {
        self.clear_high
    }

    pub fn read_nibble(&self) -> u8 {
        if self.memory_base.is_active() {
            return self.memory_base.output_nibble;
        }
        match self.device {
            ControllerDevice::Disconnected => 0x0F,
            ControllerDevice::TwoButton(_) if self.clear_high => 0,
            ControllerDevice::TwoButton(pad) => pad.read_nibble(self.select_high),
            ControllerDevice::SixButton(_) if self.clear_high => 0,
            ControllerDevice::SixButton(pad) => pad.read_nibble(self.select_high),
            ControllerDevice::Multitap(multitap) => {
                multitap.read_nibble(self.select_high, self.clear_high)
            }
            ControllerDevice::Mouse(mouse) => mouse.read_nibble(self.select_high, self.clear_high),
        }
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        write_controller_device(writer, self.device);
        writer.write_bool(self.select_high);
        writer.write_bool(self.clear_high);
        self.memory_base.write_state(writer);
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let device = read_controller_device(reader)?;
        let select_high = reader.read_bool()?;
        let clear_high = reader.read_bool()?;
        let memory_base = MemoryBase128::read_state(reader)?;
        *self = Self {
            device,
            select_high,
            clear_high,
            memory_base,
        };
        Ok(())
    }
}

fn write_controller_device(writer: &mut StateWriter, device: ControllerDevice) {
    match device {
        ControllerDevice::Disconnected => writer.write_u8(0),
        ControllerDevice::TwoButton(pad) => {
            writer.write_u8(1);
            writer.write_u8(pad.buttons.bits());
        }
        ControllerDevice::SixButton(pad) => {
            writer.write_u8(2);
            write_six_button_pad(writer, pad);
        }
        ControllerDevice::Multitap(multitap) => {
            writer.write_u8(3);
            writer.write_u8(match multitap.active_port {
                None => 0,
                Some(MultitapPort::One) => 1,
                Some(MultitapPort::Two) => 2,
                Some(MultitapPort::Three) => 3,
                Some(MultitapPort::Four) => 4,
                Some(MultitapPort::Five) => 5,
            });
            for device in multitap.ports {
                write_multitap_device(writer, device);
            }
        }
        ControllerDevice::Mouse(mouse) => {
            writer.write_u8(4);
            writer.write_u8(mouse.buttons.bits());
            writer.write_u32(mouse.pending_x as u32);
            writer.write_u32(mouse.pending_y as u32);
            writer.write_u8(mouse.latched_x);
            writer.write_u8(mouse.latched_y);
            writer.write_u8(mouse_phase_to_tag(mouse.phase));
            writer.write_bool(mouse.scan_active);
            writer.write_u64(mouse.select_elapsed);
            writer.write_u64(mouse.clear_elapsed);
        }
    }
}

fn read_controller_device(reader: &mut StateReader<'_>) -> anyhow::Result<ControllerDevice> {
    Ok(match reader.read_u8()? {
        0 => ControllerDevice::Disconnected,
        1 => ControllerDevice::TwoButton(TwoButtonPad {
            buttons: PadButtons::from_bits_retain(reader.read_u8()?),
        }),
        2 => ControllerDevice::SixButton(read_six_button_pad(reader)?),
        3 => {
            let active_port = match reader.read_u8()? {
                0 => None,
                1 => Some(MultitapPort::One),
                2 => Some(MultitapPort::Two),
                3 => Some(MultitapPort::Three),
                4 => Some(MultitapPort::Four),
                5 => Some(MultitapPort::Five),
                tag => bail!("invalid multitap active-port tag in save-state: {tag}"),
            };
            let mut ports = [MultitapDevice::Disconnected; 5];
            for port in &mut ports {
                *port = read_multitap_device(reader)?;
            }
            ControllerDevice::Multitap(FivePortMultitap { ports, active_port })
        }
        4 => ControllerDevice::Mouse(PceMouse {
            buttons: PadButtons::from_bits_retain(reader.read_u8()?),
            pending_x: reader.read_u32()? as i32,
            pending_y: reader.read_u32()? as i32,
            latched_x: reader.read_u8()?,
            latched_y: reader.read_u8()?,
            phase: tag_to_mouse_phase(reader.read_u8()?)?,
            scan_active: reader.read_bool()?,
            select_elapsed: reader.read_u64()?,
            clear_elapsed: reader.read_u64()?,
        }),
        tag => bail!("invalid controller-device tag in save-state: {tag}"),
    })
}

fn write_multitap_device(writer: &mut StateWriter, device: MultitapDevice) {
    match device {
        MultitapDevice::Disconnected => writer.write_u8(0),
        MultitapDevice::TwoButton(pad) => {
            writer.write_u8(1);
            writer.write_u8(pad.buttons.bits());
        }
        MultitapDevice::SixButton(pad) => {
            writer.write_u8(2);
            write_six_button_pad(writer, pad);
        }
    }
}

fn read_multitap_device(reader: &mut StateReader<'_>) -> anyhow::Result<MultitapDevice> {
    Ok(match reader.read_u8()? {
        0 => MultitapDevice::Disconnected,
        1 => MultitapDevice::TwoButton(TwoButtonPad {
            buttons: PadButtons::from_bits_retain(reader.read_u8()?),
        }),
        2 => MultitapDevice::SixButton(read_six_button_pad(reader)?),
        tag => bail!("invalid multitap-device tag in save-state: {tag}"),
    })
}

fn write_six_button_pad(writer: &mut StateWriter, pad: SixButtonPad) {
    writer.write_u8(pad.standard.buttons.bits());
    writer.write_u8(pad.extra_buttons.bits());
    writer.write_u8(match pad.phase {
        SixButtonPhase::Standard => 0,
        SixButtonPhase::Extended => 1,
    });
}

fn read_six_button_pad(reader: &mut StateReader<'_>) -> anyhow::Result<SixButtonPad> {
    let standard = TwoButtonPad {
        buttons: PadButtons::from_bits_retain(reader.read_u8()?),
    };
    let extra_bits = reader.read_u8()?;
    let Some(extra_buttons) = SixButtonExtraButtons::from_bits(extra_bits) else {
        bail!("invalid six-button extra-button bits in save-state: {extra_bits:#04X}");
    };
    let phase = match reader.read_u8()? {
        0 => SixButtonPhase::Standard,
        1 => SixButtonPhase::Extended,
        tag => bail!("invalid six-button phase tag in save-state: {tag}"),
    };
    Ok(SixButtonPad {
        standard,
        extra_buttons,
        phase,
    })
}

const fn mouse_phase_to_tag(phase: MouseScanPhase) -> u8 {
    match phase {
        MouseScanPhase::XHigh => 0,
        MouseScanPhase::XLow => 1,
        MouseScanPhase::YHigh => 2,
        MouseScanPhase::YLow => 3,
        MouseScanPhase::Exhausted => 4,
    }
}

fn tag_to_mouse_phase(tag: u8) -> anyhow::Result<MouseScanPhase> {
    Ok(match tag {
        0 => MouseScanPhase::XHigh,
        1 => MouseScanPhase::XLow,
        2 => MouseScanPhase::YHigh,
        3 => MouseScanPhase::YLow,
        4 => MouseScanPhase::Exhausted,
        _ => bail!("invalid mouse scan-phase tag in save-state: {tag}"),
    })
}

#[inline]
const fn multitap_device_debug_snapshot(device: MultitapDevice) -> MultitapDeviceDebugSnapshot {
    match device {
        MultitapDevice::Disconnected => MultitapDeviceDebugSnapshot::Disconnected,
        MultitapDevice::TwoButton(pad) => MultitapDeviceDebugSnapshot::TwoButton {
            buttons: pad.buttons(),
        },
        MultitapDevice::SixButton(pad) => MultitapDeviceDebugSnapshot::SixButton {
            buttons: pad.standard_pad().buttons(),
            extra_buttons: pad.extra_buttons(),
            phase: pad.phase(),
        },
    }
}
