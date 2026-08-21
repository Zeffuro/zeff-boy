use bitflags::bitflags;

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
    Multitap,
    Mouse,
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
enum MouseScanPhase {
    #[default]
    XHigh,
    XLow,
    YHigh,
    YLow,
    Exhausted,
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

pub const MULTITAP_EXHAUSTED_NIBBLE: u8 = 0x0F;

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
pub struct ControllerPort {
    device: ControllerDevice,
    select_high: bool,
    clear_high: bool,
}

impl Default for ControllerPort {
    fn default() -> Self {
        Self::new(ControllerDevice::Disconnected)
    }
}

impl ControllerPort {
    pub const fn new(device: ControllerDevice) -> Self {
        Self {
            device,
            select_high: true,
            clear_high: true,
        }
    }

    #[inline]
    pub const fn two_button() -> Self {
        Self::new(ControllerDevice::TwoButton(TwoButtonPad::new()))
    }

    #[inline]
    pub const fn six_button() -> Self {
        Self::new(ControllerDevice::SixButton(SixButtonPad::new()))
    }

    #[inline]
    pub const fn multitap(multitap: FivePortMultitap) -> Self {
        Self::new(ControllerDevice::Multitap(multitap))
    }

    #[inline]
    pub const fn mouse() -> Self {
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
    pub fn advance_master_ticks(&mut self, master_ticks: u64) {
        if let ControllerDevice::Mouse(mouse) = &mut self.device {
            mouse.advance_master_ticks(master_ticks);
        }
    }

    #[inline]
    pub fn write_lines(&mut self, select_high: bool, clear_high: bool) {
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
}
