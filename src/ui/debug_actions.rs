use crate::debug::DebugUiActions;

pub(crate) fn apply_debug_actions(
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    next_frame_requested: &mut bool,
    debug_continue_requested: &mut bool,
    backstep_requested: &mut bool,
) {
    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.next_frame_requested {
        *next_frame_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
    if actions.backstep_requested {
        *backstep_requested = true;
    }
}
