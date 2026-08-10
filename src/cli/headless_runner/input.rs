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
    for event in &opts.input_events {
        if (event.start_frame..=event.end_frame).contains(&frame) {
            input.buttons |= event.buttons;
            input.dpad |= event.dpad;
        }
        if event.reset && frame == event.start_frame {
            input.reset = true;
        }
    }
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

pub(super) fn map_host_to_nes_byte(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0F)
        | ((dpad & 0x04) << 2)
        | ((dpad & 0x08) << 2)
        | ((dpad & 0x02) << 5)
        | ((dpad & 0x01) << 7)
}
