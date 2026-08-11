use super::constants::IO_OPEN_BUS_VALUE;
use super::region::Sega8Region;

const GG_START_BUTTON_MASK: u8 = 0x80;
const GG_EXPORT_REGION_MASK: u8 = 0x40;
const IO_CONTROL_DEFAULT: u8 = 0xFF;
const IO_CONTROL_P1_TH_DIRECTION_INPUT: u8 = 0x02;
const IO_CONTROL_P2_TH_DIRECTION_INPUT: u8 = 0x08;
const IO_CONTROL_P1_TH_OUTPUT: u8 = 0x20;
const IO_CONTROL_P2_TH_OUTPUT: u8 = 0x80;
const CONTROLLER_2_P1_TH_BIT: u8 = 0x40;
const CONTROLLER_2_P2_TH_BIT: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerPort {
    One,
    Two,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    controller_1: u8,
    controller_2: u8,
    game_gear_start_pressed: bool,
    io_control: u8,
}

impl Input {
    pub fn new() -> Self {
        Self {
            controller_1: IO_OPEN_BUS_VALUE,
            controller_2: IO_OPEN_BUS_VALUE,
            game_gear_start_pressed: false,
            io_control: IO_CONTROL_DEFAULT,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn read_controller(&self, port: ControllerPort) -> u8 {
        match port {
            ControllerPort::One => self.controller_1,
            ControllerPort::Two => self.controller_2,
        }
    }

    pub fn read_controller_for_bus(&self, port: ControllerPort, region: Sega8Region) -> u8 {
        match port {
            ControllerPort::One => self.controller_1,
            ControllerPort::Two => self.controller_2_with_region_bits(region),
        }
    }

    pub fn set_controller_raw(&mut self, port: ControllerPort, value: u8) {
        match port {
            ControllerPort::One => self.controller_1 = value,
            ControllerPort::Two => self.controller_2 = value,
        }
    }

    pub fn read_game_gear_start(&self, region: Sega8Region) -> u8 {
        let mut value = IO_OPEN_BUS_VALUE;
        if self.game_gear_start_pressed {
            value &= !GG_START_BUTTON_MASK;
        }
        if !region.is_export() {
            value &= !GG_EXPORT_REGION_MASK;
        }
        value
    }

    pub fn set_game_gear_start_pressed(&mut self, pressed: bool) {
        self.game_gear_start_pressed = pressed;
    }

    pub fn game_gear_start_pressed(&self) -> bool {
        self.game_gear_start_pressed
    }

    pub fn io_control(&self) -> u8 {
        self.io_control
    }

    pub fn set_io_control(&mut self, value: u8) {
        self.io_control = value;
    }

    fn controller_2_with_region_bits(&self, region: Sega8Region) -> u8 {
        match region {
            Sega8Region::Export => self.controller_2_with_th_loopback(false),
            Sega8Region::Japanese => self.controller_2,
            Sega8Region::JapanesePowerBaseConverter => self.controller_2_with_th_loopback(true),
        }
    }

    fn controller_2_with_th_loopback(&self, inverted: bool) -> u8 {
        let mut value = self.controller_2;
        value = self.apply_th_loopback(
            value,
            IO_CONTROL_P1_TH_DIRECTION_INPUT,
            CONTROLLER_2_P1_TH_BIT,
            IO_CONTROL_P1_TH_OUTPUT,
            inverted,
        );
        self.apply_th_loopback(
            value,
            IO_CONTROL_P2_TH_DIRECTION_INPUT,
            CONTROLLER_2_P2_TH_BIT,
            IO_CONTROL_P2_TH_OUTPUT,
            inverted,
        )
    }

    fn apply_th_loopback(
        &self,
        value: u8,
        direction_input: u8,
        input_bit: u8,
        output_bit: u8,
        inverted: bool,
    ) -> u8 {
        if self.io_control & direction_input != 0 {
            return value;
        }
        let high = (self.io_control & output_bit != 0) ^ inverted;
        apply_bit(value, input_bit, high)
    }
}

fn apply_bit(value: u8, input_bit: u8, high: bool) -> u8 {
    if high {
        value | input_bit
    } else {
        value & !input_bit
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controllers_default_to_released_active_low_bits() {
        let input = Input::new();

        assert_eq!(input.read_controller(ControllerPort::One), 0xFF);
        assert_eq!(input.read_controller(ControllerPort::Two), 0xFF);
        assert_eq!(input.read_game_gear_start(Sega8Region::Export), 0xFF);
        assert_eq!(input.io_control(), IO_CONTROL_DEFAULT);
    }

    #[test]
    fn raw_controller_state_can_be_updated() {
        let mut input = Input::new();

        input.set_controller_raw(ControllerPort::One, 0xF0);

        assert_eq!(input.read_controller(ControllerPort::One), 0xF0);
    }

    #[test]
    fn game_gear_start_is_active_low_on_bit_7() {
        let mut input = Input::new();

        input.set_game_gear_start_pressed(true);

        assert_eq!(input.read_game_gear_start(Sega8Region::Export), 0x7F);
        assert!(input.game_gear_start_pressed());
    }

    #[test]
    fn game_gear_region_is_export_high_japanese_low_on_bit_6() {
        let input = Input::new();

        assert_eq!(
            input.read_game_gear_start(Sega8Region::Export) & GG_EXPORT_REGION_MASK,
            GG_EXPORT_REGION_MASK
        );
        assert_eq!(
            input.read_game_gear_start(Sega8Region::Japanese) & GG_EXPORT_REGION_MASK,
            0
        );
    }

    #[test]
    fn export_sms_io_control_reflects_th_outputs_on_controller_port_2() {
        let mut input = Input::new();

        input.set_io_control(0xF5);
        assert_eq!(
            input.read_controller_for_bus(ControllerPort::Two, Sega8Region::Export) & 0xC0,
            0xC0
        );

        input.set_io_control(0x55);
        assert_eq!(
            input.read_controller_for_bus(ControllerPort::Two, Sega8Region::Export) & 0xC0,
            0x00
        );
    }

    #[test]
    fn japanese_sms_io_control_does_not_reflect_th_outputs() {
        let mut input = Input::new();

        input.set_controller_raw(ControllerPort::Two, 0xC0);
        input.set_io_control(0x55);

        assert_eq!(
            input.read_controller_for_bus(ControllerPort::Two, Sega8Region::Japanese) & 0xC0,
            0xC0
        );
    }

    #[test]
    fn japanese_power_base_converter_inverts_th_outputs() {
        let mut input = Input::new();

        input.set_io_control(0xF5);
        assert_eq!(
            input.read_controller_for_bus(
                ControllerPort::Two,
                Sega8Region::JapanesePowerBaseConverter
            ) & 0xC0,
            0x00
        );

        input.set_io_control(0x55);
        assert_eq!(
            input.read_controller_for_bus(
                ControllerPort::Two,
                Sega8Region::JapanesePowerBaseConverter
            ) & 0xC0,
            0xC0
        );
    }
}
