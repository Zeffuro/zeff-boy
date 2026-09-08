use crate::settings::AutofirePattern;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct AutofireState {
    pub(super) legacy_phase: u8,
    pub(super) configured: [[u8; 8]; 5],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AutofirePreview {
    pub(super) buttons: Vec<[u8; 5]>,
    pub(super) states: Vec<AutofireState>,
}

pub(super) fn preview(
    state: AutofireState,
    legacy_held: bool,
    configured: [[Option<AutofirePattern>; 8]; 5],
    buttons: &[[u8; 5]],
) -> AutofirePreview {
    let mut state = state;
    let mut transformed = Vec::with_capacity(buttons.len());
    let mut states = Vec::with_capacity(buttons.len());
    for buttons in buttons {
        transformed.push(apply_frame(&mut state, legacy_held, configured, *buttons));
        states.push(state);
    }
    AutofirePreview {
        buttons: transformed,
        states,
    }
}

pub(super) fn reset_released(
    state: &mut AutofireState,
    legacy_held: bool,
    configured: [[Option<AutofirePattern>; 8]; 5],
    buttons: [u8; 5],
) {
    if !legacy_held {
        state.legacy_phase = 0;
    }
    for (player, patterns) in configured.iter().enumerate() {
        for (action, pattern) in patterns.iter().enumerate() {
            let bit = 1 << action_bit(action);
            if pattern.is_none() || buttons[player] & bit == 0 {
                state.configured[player][action] = 0;
            }
        }
    }
}

pub(super) fn needs_staging(
    legacy_held: bool,
    configured: [[Option<AutofirePattern>; 8]; 5],
    buttons: [u8; 5],
) -> bool {
    legacy_held
        || (0..5).any(|player| {
            (0..8).any(|action| {
                configured[player][action].is_some()
                    && buttons[player] & (1 << action_bit(action)) != 0
            })
        })
}

fn apply_frame(
    state: &mut AutofireState,
    legacy_held: bool,
    configured: [[Option<AutofirePattern>; 8]; 5],
    mut buttons: [u8; 5],
) -> [u8; 5] {
    for (player, patterns) in configured.iter().enumerate() {
        for (action, pattern) in patterns.iter().enumerate() {
            let bit = 1 << action_bit(action);
            let Some(pattern) = *pattern else {
                state.configured[player][action] = 0;
                continue;
            };
            if buttons[player] & bit == 0 {
                state.configured[player][action] = 0;
                continue;
            }
            let phase = state.configured[player][action];
            if phase % pattern.period_frames >= pattern.on_frames {
                buttons[player] &= !bit;
            }
            state.configured[player][action] = (phase + 1) % pattern.period_frames;
        }
    }

    if legacy_held {
        state.legacy_phase = state.legacy_phase.wrapping_add(1);
        if state.legacy_phase % 2 == 1 {
            buttons[0] = 0;
        }
    } else {
        state.legacy_phase = 0;
    }
    buttons
}

const fn action_bit(action: usize) -> u8 {
    match action {
        0 => 0,
        1 => 1,
        2 => 6,
        3 => 7,
        4 => 4,
        5 => 5,
        6 => 2,
        7 => 3,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern() -> AutofirePattern {
        AutofirePattern {
            period_frames: 2,
            on_frames: 1,
        }
    }

    #[test]
    fn configured_autofire_advances_only_held_target_and_resets_on_release() {
        let mut configured = [[None; 8]; 5];
        configured[1][0] = Some(pattern());
        let preview = preview(
            AutofireState::default(),
            false,
            configured,
            &[[0, 1, 0, 0, 0], [0, 1, 0, 0, 0], [0; 5]],
        );
        assert_eq!(preview.buttons, vec![[0, 1, 0, 0, 0], [0; 5], [0; 5]]);
        assert_eq!(preview.states[0].configured[1][0], 1);
        assert_eq!(preview.states[1].configured[1][0], 0);
        assert_eq!(preview.states[2].configured[1][0], 0);
    }

    #[test]
    fn configured_patterns_leave_unselected_buttons_and_dpad_alone() {
        let mut configured = [[None; 8]; 5];
        configured[0][1] = Some(pattern());
        let preview = preview(
            AutofireState::default(),
            false,
            configured,
            &[[0b0010_0101, 0, 0, 0, 0]],
        );
        assert_eq!(preview.buttons, vec![[0b0010_0101, 0, 0, 0, 0]]);
    }

    #[test]
    fn legacy_turbo_preserves_its_player_one_whole_mask_contract() {
        let preview = preview(
            AutofireState::default(),
            true,
            [[None; 8]; 5],
            &[[1, 2, 3, 4, 5], [1, 2, 3, 4, 5]],
        );
        assert_eq!(preview.buttons, vec![[0, 2, 3, 4, 5], [1, 2, 3, 4, 5]]);
        assert_eq!(preview.states[1].legacy_phase, 2);
    }

    #[test]
    fn configured_patterns_do_not_jump_when_their_period_does_not_divide_256() {
        for period_frames in [3, 5, 60] {
            let mut configured = [[None; 8]; 5];
            configured[0][0] = Some(AutofirePattern {
                period_frames,
                on_frames: period_frames - 1,
            });
            let frames = vec![[1, 0, 0, 0, 0]; 605];
            let batch = preview(AutofireState::default(), false, configured, &frames);
            let mut state = AutofireState::default();
            let mut singles = Vec::new();
            for frame in frames {
                let one = preview(state, false, configured, &[frame]);
                singles.extend(one.buttons);
                state = one.states[0];
            }
            assert_eq!(batch.buttons, singles);
            assert_eq!(batch.states.last(), Some(&state));
            assert_eq!(
                state.configured[0][0],
                (605 % usize::from(period_frames)) as u8
            );
        }
    }

    #[test]
    fn no_staging_is_needed_without_a_held_autofire_target() {
        let mut configured = [[None; 8]; 5];
        configured[0][0] = Some(pattern());
        assert!(!needs_staging(false, configured, [0; 5]));
        assert!(needs_staging(false, configured, [1, 0, 0, 0, 0]));
    }
}
