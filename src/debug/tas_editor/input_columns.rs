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
        "sms" | "master_system" | "sg" | "sg1000" | "sg-1000" => {
            let mut columns = direction_columns();
            columns.extend(button_columns(&[("1", 0), ("2", 1)]));
            columns
        }
        "gg" | "game_gear" => {
            let mut columns = direction_columns();
            columns.extend(button_columns(&[("1", 0), ("2", 1), ("St", 3)]));
            columns
        }
        "ws" | "wonderswan" => wonderswan_columns(),
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

fn wonderswan_columns() -> Vec<DigitalColumn> {
    [("X1", 0), ("X2", 1), ("X3", 2), ("X4", 3)]
        .into_iter()
        .map(|(label, bit)| DigitalColumn {
            label,
            field: DigitalField::Dpad,
            mask: 1 << bit,
        })
        .chain(
            [
                ("A", 0),
                ("B", 1),
                ("St", 3),
                ("Y1", 4),
                ("Y2", 5),
                ("Y3", 6),
                ("Y4", 7),
            ]
            .into_iter()
            .map(|(label, bit)| DigitalColumn {
                label,
                field: DigitalField::Buttons,
                mask: 1 << bit,
            }),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_system_rows_show_the_two_standard_pad_buttons() {
        assert_eq!(
            digital_columns("sms"),
            vec![
                DigitalColumn {
                    label: "R",
                    field: DigitalField::Dpad,
                    mask: 1,
                },
                DigitalColumn {
                    label: "L",
                    field: DigitalField::Dpad,
                    mask: 2,
                },
                DigitalColumn {
                    label: "U",
                    field: DigitalField::Dpad,
                    mask: 4,
                },
                DigitalColumn {
                    label: "D",
                    field: DigitalField::Dpad,
                    mask: 8,
                },
                DigitalColumn {
                    label: "1",
                    field: DigitalField::Buttons,
                    mask: 1,
                },
                DigitalColumn {
                    label: "2",
                    field: DigitalField::Buttons,
                    mask: 2,
                },
            ]
        );
    }

    #[test]
    fn sg1000_rows_show_the_two_standard_pad_buttons() {
        assert_eq!(digital_columns("sg"), digital_columns("sms"));
    }

    #[test]
    fn game_gear_rows_show_the_built_in_pad_and_start() {
        let columns = digital_columns("game_gear");
        assert_eq!(columns.len(), 7);
        assert_eq!(columns[4].label, "1");
        assert_eq!(columns[5].label, "2");
        assert_eq!(columns[6].label, "St");
        assert_eq!(columns[6].mask, 0x08);
    }

    #[test]
    fn wonderswan_rows_show_the_x_y_and_action_controls() {
        assert_eq!(
            digital_columns("ws"),
            vec![
                DigitalColumn {
                    label: "X1",
                    field: DigitalField::Dpad,
                    mask: 1
                },
                DigitalColumn {
                    label: "X2",
                    field: DigitalField::Dpad,
                    mask: 2
                },
                DigitalColumn {
                    label: "X3",
                    field: DigitalField::Dpad,
                    mask: 4
                },
                DigitalColumn {
                    label: "X4",
                    field: DigitalField::Dpad,
                    mask: 8
                },
                DigitalColumn {
                    label: "A",
                    field: DigitalField::Buttons,
                    mask: 1
                },
                DigitalColumn {
                    label: "B",
                    field: DigitalField::Buttons,
                    mask: 2
                },
                DigitalColumn {
                    label: "St",
                    field: DigitalField::Buttons,
                    mask: 8
                },
                DigitalColumn {
                    label: "Y1",
                    field: DigitalField::Buttons,
                    mask: 16
                },
                DigitalColumn {
                    label: "Y2",
                    field: DigitalField::Buttons,
                    mask: 32
                },
                DigitalColumn {
                    label: "Y3",
                    field: DigitalField::Buttons,
                    mask: 64
                },
                DigitalColumn {
                    label: "Y4",
                    field: DigitalField::Buttons,
                    mask: 128
                },
            ]
        );
    }
}
