use crate::tas_project::{TasColecoControllerInput, TasColecoKeypadKey, TasEditorSession};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ColecoControl {
    Up,
    Right,
    Down,
    Left,
    LeftButton,
    RightButton,
}

pub(super) const COLECO_CONTROLS: [(ColecoControl, &str); 6] = [
    (ColecoControl::Up, "U"),
    (ColecoControl::Right, "R"),
    (ColecoControl::Down, "D"),
    (ColecoControl::Left, "L"),
    (ColecoControl::LeftButton, "L1"),
    (ColecoControl::RightButton, "R1"),
];

pub(super) const COLECO_KEYPAD_KEYS: [TasColecoKeypadKey; 13] = [
    TasColecoKeypadKey::None,
    TasColecoKeypadKey::Zero,
    TasColecoKeypadKey::One,
    TasColecoKeypadKey::Two,
    TasColecoKeypadKey::Three,
    TasColecoKeypadKey::Four,
    TasColecoKeypadKey::Five,
    TasColecoKeypadKey::Six,
    TasColecoKeypadKey::Seven,
    TasColecoKeypadKey::Eight,
    TasColecoKeypadKey::Nine,
    TasColecoKeypadKey::Star,
    TasColecoKeypadKey::Pound,
];

pub(super) fn is_coleco_project(session: &TasEditorSession) -> bool {
    matches!(
        session.project().identity().system.as_str(),
        "coleco" | "colecovision"
    )
}

pub(super) fn control_pressed(input: TasColecoControllerInput, control: ColecoControl) -> bool {
    match control {
        ColecoControl::Up => input.up,
        ColecoControl::Right => input.right,
        ColecoControl::Down => input.down,
        ColecoControl::Left => input.left,
        ColecoControl::LeftButton => input.left_button,
        ColecoControl::RightButton => input.right_button,
    }
}

pub(super) fn set_control(
    input: &mut TasColecoControllerInput,
    control: ColecoControl,
    pressed: bool,
) {
    match control {
        ColecoControl::Up => input.up = pressed,
        ColecoControl::Right => input.right = pressed,
        ColecoControl::Down => input.down = pressed,
        ColecoControl::Left => input.left = pressed,
        ColecoControl::LeftButton => input.left_button = pressed,
        ColecoControl::RightButton => input.right_button = pressed,
    }
}

pub(super) const fn keypad_label(key: TasColecoKeypadKey) -> &'static str {
    match key {
        TasColecoKeypadKey::None => "—",
        TasColecoKeypadKey::Zero => "0",
        TasColecoKeypadKey::One => "1",
        TasColecoKeypadKey::Two => "2",
        TasColecoKeypadKey::Three => "3",
        TasColecoKeypadKey::Four => "4",
        TasColecoKeypadKey::Five => "5",
        TasColecoKeypadKey::Six => "6",
        TasColecoKeypadKey::Seven => "7",
        TasColecoKeypadKey::Eight => "8",
        TasColecoKeypadKey::Nine => "9",
        TasColecoKeypadKey::Star => "*",
        TasColecoKeypadKey::Pound => "#",
    }
}
