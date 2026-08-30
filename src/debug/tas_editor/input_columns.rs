use crate::tas_project::TasEditorSession;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DigitalField {
    Buttons,
    Dpad,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DigitalColumn {
    pub(super) label: &'static str,
    pub(super) field: DigitalField,
    pub(super) mask: u8,
}

pub(super) fn applicable_player_count(session: &TasEditorSession) -> usize {
    session
        .project()
        .identity()
        .devices
        .iter()
        .filter_map(|device| player_number(&device.port))
        .max()
        .unwrap_or(1)
}

pub(super) fn player_number(port: &str) -> Option<usize> {
    let suffix = port.strip_prefix('p')?;
    let player = suffix.parse::<usize>().ok()?;
    (1..=5).contains(&player).then_some(player)
}

pub(super) fn digital_columns(system: &str) -> Vec<DigitalColumn> {
    match system {
        "gb" | "game_boy" | "nes" => {
            let mut columns = direction_columns();
            columns.extend(button_columns(&[("A", 0), ("B", 1), ("Sel", 2), ("St", 3)]));
            columns
        }
        "gba" | "game_boy_advance" => {
            let mut columns = direction_columns();
            columns.extend(button_columns(&[
                ("A", 0),
                ("B", 1),
                ("Sel", 2),
                ("St", 3),
                ("L", 4),
                ("R", 5),
            ]));
            columns
        }
        "pce" | "pc_engine" => {
            let mut columns = direction_columns();
            columns.extend(button_columns(&[
                ("I", 0),
                ("II", 1),
                ("Sel", 2),
                ("Run", 3),
                ("III", 4),
                ("IV", 5),
                ("V", 6),
                ("VI", 7),
            ]));
            columns
        }
        _ => raw_digital_columns(),
    }
}

fn direction_columns() -> Vec<DigitalColumn> {
    [("R", 0), ("L", 1), ("U", 2), ("D", 3)]
        .into_iter()
        .map(|(label, bit)| DigitalColumn {
            label,
            field: DigitalField::Dpad,
            mask: 1 << bit,
        })
        .collect()
}

fn raw_digital_columns() -> Vec<DigitalColumn> {
    const DPAD_LABELS: [&str; 8] = ["D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7"];
    const BUTTON_LABELS: [&str; 8] = ["B0", "B1", "B2", "B3", "B4", "B5", "B6", "B7"];
    DPAD_LABELS
        .into_iter()
        .enumerate()
        .map(|(bit, label)| DigitalColumn {
            label,
            field: DigitalField::Dpad,
            mask: 1 << bit,
        })
        .chain(
            BUTTON_LABELS
                .into_iter()
                .enumerate()
                .map(|(bit, label)| DigitalColumn {
                    label,
                    field: DigitalField::Buttons,
                    mask: 1 << bit,
                }),
        )
        .collect()
}

fn button_columns(columns: &[(&'static str, u8)]) -> Vec<DigitalColumn> {
    columns
        .iter()
        .map(|&(label, bit)| DigitalColumn {
            label,
            field: DigitalField::Buttons,
            mask: 1 << bit,
        })
        .collect()
}
