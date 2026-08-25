use crate::cli::types::HeadlessInputEvent;

use super::HeadlessOptions;

#[derive(Clone, Copy, Default)]
pub(super) struct InputMasks {
    pub(super) buttons: u8,
    pub(super) dpad: u8,
    pub(super) reset: bool,
    pub(super) zapper_enabled: bool,
    pub(super) zapper_trigger: bool,
    pub(super) zapper_hit: bool,
    pub(super) zapper_screen_pos: Option<(u16, u16)>,
}

pub(super) fn input_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    let mut input = InputMasks {
        zapper_enabled: !opts.zapper_events.is_empty(),
        zapper_screen_pos: opts.zapper_events.first().map(|event| (event.x, event.y)),
        ..InputMasks::default()
    };
    apply_input_events(&mut input, &opts.input_events, frame);
    for event in &opts.zapper_events {
        if (event.start_frame..=event.end_frame).contains(&frame) {
            input.zapper_enabled = true;
            input.zapper_trigger |= event.trigger;
            input.zapper_hit |= event.hit;
            input.zapper_screen_pos = Some((event.x, event.y));
        }
    }
    input
}

pub(super) fn input_p2_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    input_for_player_frame(&opts.input_events_p2, frame)
}

pub(super) fn input_p3_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    input_for_player_frame(&opts.input_events_p3, frame)
}

pub(super) fn input_p4_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    input_for_player_frame(&opts.input_events_p4, frame)
}

pub(super) fn input_p5_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    input_for_player_frame(&opts.input_events_p5, frame)
}

fn input_for_player_frame(events: &[HeadlessInputEvent], frame: u64) -> InputMasks {
    let mut input = InputMasks::default();
    apply_input_events(&mut input, events, frame);
    input
}

fn apply_input_events(input: &mut InputMasks, events: &[HeadlessInputEvent], frame: u64) {
    for event in events {
        if (event.start_frame..=event.end_frame).contains(&frame) {
            input.buttons |= event.buttons;
            input.dpad |= event.dpad;
        }
        if event.reset && frame == event.start_frame {
            input.reset = true;
        }
    }
}
