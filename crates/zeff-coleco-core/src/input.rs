use crate::constants::{
    JOYSTICK_MODE_PORT_END, JOYSTICK_MODE_PORT_START, KEYPAD_MODE_PORT_END, KEYPAD_MODE_PORT_START,
};

const CONTROLLER_IDLE_VALUE: u8 = 0x7F;
const KEYPAD_CODE_MASK: u8 = 0x0F;
const UP_MASK: u8 = 0x01;
const RIGHT_MASK: u8 = 0x02;
const DOWN_MASK: u8 = 0x04;
const LEFT_MASK: u8 = 0x08;
const BUTTON_MASK: u8 = 0x40;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerMux {
    #[default]
    Keypad,
    Joystick,
}

impl ControllerMux {
    pub const fn from_output_port(port: u8) -> Option<Self> {
        match port {
            KEYPAD_MODE_PORT_START..=KEYPAD_MODE_PORT_END => Some(Self::Keypad),
            JOYSTICK_MODE_PORT_START..=JOYSTICK_MODE_PORT_END => Some(Self::Joystick),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeypadKey {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Star,
    Pound,
}

impl KeypadKey {
    pub const fn code(self) -> u8 {
        match self {
            Self::Zero => 0xA,
            Self::One => 0xD,
            Self::Two => 0x7,
            Self::Three => 0xC,
            Self::Four => 0x2,
            Self::Five => 0x3,
            Self::Six => 0xE,
            Self::Seven => 0x5,
            Self::Eight => 0x1,
            Self::Nine => 0xB,
            Self::Star => 0x9,
            Self::Pound => 0x6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StandardController {
    pub up: bool,
    pub right: bool,
    pub down: bool,
    pub left: bool,
    pub left_button: bool,
    pub right_button: bool,
    pub keypad: Option<KeypadKey>,
}

impl StandardController {
    pub const fn read(self, mux: ControllerMux) -> u8 {
        match mux {
            ControllerMux::Keypad => self.read_keypad(),
            ControllerMux::Joystick => self.read_joystick(),
        }
    }

    const fn read_keypad(self) -> u8 {
        let mut value = CONTROLLER_IDLE_VALUE;
        if let Some(key) = self.keypad {
            value = (value & !KEYPAD_CODE_MASK) | key.code();
        }
        if self.right_button {
            value &= !BUTTON_MASK;
        }
        value
    }

    const fn read_joystick(self) -> u8 {
        let mut value = CONTROLLER_IDLE_VALUE;
        if self.up {
            value &= !UP_MASK;
        }
        if self.right {
            value &= !RIGHT_MASK;
        }
        if self.down {
            value &= !DOWN_MASK;
        }
        if self.left {
            value &= !LEFT_MASK;
        }
        if self.left_button {
            value &= !BUTTON_MASK;
        }
        value
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControllerPorts {
    mux: ControllerMux,
    players: [StandardController; 2],
}

impl ControllerPorts {
    pub const fn new() -> Self {
        Self {
            mux: ControllerMux::Keypad,
            players: [StandardController {
                up: false,
                right: false,
                down: false,
                left: false,
                left_button: false,
                right_button: false,
                keypad: None,
            }; 2],
        }
    }

    pub const fn mux(&self) -> ControllerMux {
        self.mux
    }

    pub fn write_output_port(&mut self, port: u8) {
        if let Some(mux) = ControllerMux::from_output_port(port) {
            self.mux = mux;
        }
    }

    pub const fn player(&self, player: usize) -> Option<StandardController> {
        match player {
            0 | 1 => Some(self.players[player]),
            _ => None,
        }
    }

    pub fn player_mut(&mut self, player: usize) -> Option<&mut StandardController> {
        self.players.get_mut(player)
    }

    pub const fn read_player(&self, player: usize) -> Option<u8> {
        match player {
            0 | 1 => Some(self.players[player].read(self.mux)),
            _ => None,
        }
    }

    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        w.write_u8(match self.mux {
            ControllerMux::Keypad => 0,
            ControllerMux::Joystick => 1,
        });
        for player in self.players {
            w.write_bool(player.up);
            w.write_bool(player.right);
            w.write_bool(player.down);
            w.write_bool(player.left);
            w.write_bool(player.left_button);
            w.write_bool(player.right_button);
            w.write_u8(keypad_to_tag(player.keypad));
        }
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.mux = match r.read_u8()? {
            0 => ControllerMux::Keypad,
            1 => ControllerMux::Joystick,
            tag => anyhow::bail!("invalid ColecoVision controller mux tag: {tag}"),
        };
        for player in &mut self.players {
            player.up = r.read_bool()?;
            player.right = r.read_bool()?;
            player.down = r.read_bool()?;
            player.left = r.read_bool()?;
            player.left_button = r.read_bool()?;
            player.right_button = r.read_bool()?;
            player.keypad = tag_to_keypad(r.read_u8()?)?;
        }
        Ok(())
    }
}

const fn keypad_to_tag(key: Option<KeypadKey>) -> u8 {
    match key {
        None => 0,
        Some(KeypadKey::Zero) => 1,
        Some(KeypadKey::One) => 2,
        Some(KeypadKey::Two) => 3,
        Some(KeypadKey::Three) => 4,
        Some(KeypadKey::Four) => 5,
        Some(KeypadKey::Five) => 6,
        Some(KeypadKey::Six) => 7,
        Some(KeypadKey::Seven) => 8,
        Some(KeypadKey::Eight) => 9,
        Some(KeypadKey::Nine) => 10,
        Some(KeypadKey::Star) => 11,
        Some(KeypadKey::Pound) => 12,
    }
}

fn tag_to_keypad(tag: u8) -> anyhow::Result<Option<KeypadKey>> {
    Ok(match tag {
        0 => None,
        1 => Some(KeypadKey::Zero),
        2 => Some(KeypadKey::One),
        3 => Some(KeypadKey::Two),
        4 => Some(KeypadKey::Three),
        5 => Some(KeypadKey::Four),
        6 => Some(KeypadKey::Five),
        7 => Some(KeypadKey::Six),
        8 => Some(KeypadKey::Seven),
        9 => Some(KeypadKey::Eight),
        10 => Some(KeypadKey::Nine),
        11 => Some(KeypadKey::Star),
        12 => Some(KeypadKey::Pound),
        _ => anyhow::bail!("invalid ColecoVision keypad tag: {tag}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypad_codes_match_the_standard_controller_wiring() {
        let cases = [
            (KeypadKey::Zero, 0xA),
            (KeypadKey::One, 0xD),
            (KeypadKey::Two, 0x7),
            (KeypadKey::Three, 0xC),
            (KeypadKey::Four, 0x2),
            (KeypadKey::Five, 0x3),
            (KeypadKey::Six, 0xE),
            (KeypadKey::Seven, 0x5),
            (KeypadKey::Eight, 0x1),
            (KeypadKey::Nine, 0xB),
            (KeypadKey::Star, 0x9),
            (KeypadKey::Pound, 0x6),
        ];

        for (key, expected_code) in cases {
            let controller = StandardController {
                keypad: Some(key),
                ..StandardController::default()
            };
            assert_eq!(controller.read(ControllerMux::Keypad), 0x70 | expected_code);
        }
    }

    #[test]
    fn keypad_view_keeps_upper_bits_and_uses_the_right_button() {
        let controller = StandardController {
            right_button: true,
            keypad: Some(KeypadKey::Five),
            ..StandardController::default()
        };

        assert_eq!(
            StandardController::default().read(ControllerMux::Keypad),
            0x7F
        );
        assert_eq!(controller.read(ControllerMux::Keypad), 0x33);
    }

    #[test]
    fn joystick_view_encodes_directions_and_the_left_button_active_low() {
        let controller = StandardController {
            up: true,
            right: true,
            down: true,
            left: true,
            left_button: true,
            right_button: true,
            keypad: Some(KeypadKey::One),
        };

        assert_eq!(controller.read(ControllerMux::Joystick), 0x30);
        assert_eq!(controller.read(ControllerMux::Keypad), 0x3D);
    }

    #[test]
    fn output_port_ranges_select_the_shared_controller_mux() {
        let mut ports = ControllerPorts::new();
        assert_eq!(ports.mux(), ControllerMux::Keypad);

        ports.write_output_port(0xC0);
        assert_eq!(ports.mux(), ControllerMux::Joystick);
        ports.write_output_port(0xDF);
        assert_eq!(ports.mux(), ControllerMux::Joystick);
        ports.write_output_port(0x80);
        assert_eq!(ports.mux(), ControllerMux::Keypad);
        ports.write_output_port(0x9F);
        assert_eq!(ports.mux(), ControllerMux::Keypad);
        ports.write_output_port(0xA0);
        assert_eq!(ports.mux(), ControllerMux::Keypad);
    }

    #[test]
    fn players_are_independent_while_sharing_the_mux_selection() {
        let mut ports = ControllerPorts::new();
        ports.player_mut(0).unwrap().keypad = Some(KeypadKey::One);
        ports.player_mut(1).unwrap().keypad = Some(KeypadKey::Pound);

        assert_eq!(ports.read_player(0), Some(0x7D));
        assert_eq!(ports.read_player(1), Some(0x76));

        ports.player_mut(0).unwrap().up = true;
        ports.player_mut(0).unwrap().left_button = true;
        ports.player_mut(1).unwrap().right = true;
        ports.player_mut(1).unwrap().left_button = false;
        ports.write_output_port(0xC0);

        assert_eq!(ports.read_player(0), Some(0x3E));
        assert_eq!(ports.read_player(1), Some(0x7D));
        assert_eq!(ports.read_player(2), None);
    }
}
