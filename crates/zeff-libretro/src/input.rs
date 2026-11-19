use crate::api::*;
use crate::callbacks::{CB_INPUT_STATE, lock};
use std::os::raw::c_uint;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputDescriptorConfig {
    pub(crate) md_layout: bool,
    pub(crate) supports_shoulders: bool,
    pub(crate) supports_p2_joypad: bool,
    pub(crate) supports_p2_lightgun: bool,
}

pub(crate) fn poll_joypad_port(port: c_uint) -> (u8, u8) {
    let cb_state = *lock(&CB_INPUT_STATE);
    let query = |id: c_uint| -> bool {
        cb_state.is_some_and(|cb| unsafe { cb(port, RETRO_DEVICE_JOYPAD, 0, id) != 0 })
    };

    let mut buttons: u8 = 0;
    let mut dpad: u8 = 0;

    if query(RETRO_DEVICE_ID_JOYPAD_A) {
        buttons |= 0x01;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_B) {
        buttons |= 0x02;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_SELECT) {
        buttons |= 0x04;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_START) {
        buttons |= 0x08;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_L) {
        buttons |= 0x10;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_R) {
        buttons |= 0x20;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_RIGHT) {
        dpad |= 0x01;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_LEFT) {
        dpad |= 0x02;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_UP) {
        dpad |= 0x04;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_DOWN) {
        dpad |= 0x08;
    }

    (buttons, dpad)
}

pub(crate) fn poll_lightgun_port(port: c_uint) -> (bool, bool) {
    let cb_state = *lock(&CB_INPUT_STATE);
    let query = |id: c_uint| -> bool {
        cb_state.is_some_and(|cb| unsafe { cb(port, RETRO_DEVICE_LIGHTGUN, 0, id) != 0 })
    };
    let trigger = query(RETRO_DEVICE_ID_LIGHTGUN_TRIGGER);
    let offscreen = query(RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN);
    let hit = trigger && !offscreen;
    (trigger, hit)
}

pub(crate) fn set_input_descriptors(config: InputDescriptorConfig) {
    use crate::callbacks::env_cmd;
    use std::os::raw::c_void;

    let mut descs = Vec::new();
    if config.md_layout {
        set_md_input_descriptors();
        return;
    }
    push_joypad_descriptors(&mut descs, 0, config.supports_shoulders);

    if config.supports_p2_joypad {
        push_joypad_descriptors(&mut descs, 1, false);
    }

    if config.supports_p2_lightgun {
        descs.extend_from_slice(&[
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_LIGHTGUN,
                index: 0,
                id: RETRO_DEVICE_ID_LIGHTGUN_TRIGGER,
                description: c"Zapper Trigger".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_LIGHTGUN,
                index: 0,
                id: RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN,
                description: c"Zapper Off-screen".as_ptr(),
            },
        ]);
    }

    descs.push(retro_input_descriptor {
        port: 0,
        device: RETRO_DEVICE_NONE,
        index: 0,
        id: 0,
        description: std::ptr::null(),
    });

    env_cmd(
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
        descs.as_ptr() as *mut c_void,
    );
}

fn push_joypad_descriptors(
    descs: &mut Vec<retro_input_descriptor>,
    port: c_uint,
    include_shoulders: bool,
) {
    descs.extend_from_slice(&[
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: c"A".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_B,
            description: c"B".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_SELECT,
            description: c"Select".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_START,
            description: c"Start".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_UP,
            description: c"D-Pad Up".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_DOWN,
            description: c"D-Pad Down".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_LEFT,
            description: c"D-Pad Left".as_ptr(),
        },
        retro_input_descriptor {
            port,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_RIGHT,
            description: c"D-Pad Right".as_ptr(),
        },
    ]);

    if include_shoulders {
        descs.extend_from_slice(&[
            retro_input_descriptor {
                port,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_L,
                description: c"L".as_ptr(),
            },
            retro_input_descriptor {
                port,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_R,
                description: c"R".as_ptr(),
            },
        ]);
    }
}

pub(crate) const MD_SIX_BUTTON_DEVICE: u32 = RETRO_DEVICE_JOYPAD | (1 << 8);

pub(crate) fn poll_md_joypad_port(port: c_uint) -> (u8, u8) {
    let (mut buttons, dpad) = poll_joypad_port(port);
    buttons = (buttons & !3) | ((buttons & 1) << 1) | ((buttons & 2) >> 1);
    let cb = *lock(&CB_INPUT_STATE);
    for (id, bit) in [(RETRO_DEVICE_ID_JOYPAD_Y, 6), (RETRO_DEVICE_ID_JOYPAD_X, 7)] {
        if cb.is_some_and(|cb| unsafe { cb(port, RETRO_DEVICE_JOYPAD, 0, id) != 0 }) {
            buttons |= 1 << bit;
        }
    }
    (buttons, dpad)
}

fn set_md_input_descriptors() {
    const TYPES: &[retro_controller_description] = &[
        retro_controller_description {
            desc: c"MD 3-button pad".as_ptr(),
            id: RETRO_DEVICE_JOYPAD,
        },
        retro_controller_description {
            desc: c"MD 6-button pad".as_ptr(),
            id: MD_SIX_BUTTON_DEVICE,
        },
    ];
    const CONTROLLERS: &[retro_controller_info] = &[
        retro_controller_info {
            types: TYPES.as_ptr(),
            num_types: TYPES.len() as u32,
        },
        retro_controller_info {
            types: TYPES.as_ptr(),
            num_types: TYPES.len() as u32,
        },
        retro_controller_info {
            types: std::ptr::null(),
            num_types: 0,
        },
    ];
    const DESCRIPTORS: [retro_input_descriptor; 25] = {
        let buttons = [
            (RETRO_DEVICE_ID_JOYPAD_Y, c"A"),
            (RETRO_DEVICE_ID_JOYPAD_B, c"B"),
            (RETRO_DEVICE_ID_JOYPAD_A, c"C"),
            (RETRO_DEVICE_ID_JOYPAD_L, c"X"),
            (RETRO_DEVICE_ID_JOYPAD_X, c"Y"),
            (RETRO_DEVICE_ID_JOYPAD_R, c"Z"),
            (RETRO_DEVICE_ID_JOYPAD_SELECT, c"Mode (6-button)"),
            (RETRO_DEVICE_ID_JOYPAD_START, c"Start"),
            (RETRO_DEVICE_ID_JOYPAD_UP, c"Up"),
            (RETRO_DEVICE_ID_JOYPAD_DOWN, c"Down"),
            (RETRO_DEVICE_ID_JOYPAD_LEFT, c"Left"),
            (RETRO_DEVICE_ID_JOYPAD_RIGHT, c"Right"),
        ];
        let mut descriptors = [const {
            retro_input_descriptor {
                port: 0,
                device: RETRO_DEVICE_NONE,
                index: 0,
                id: 0,
                description: std::ptr::null(),
            }
        }; 25];
        let mut index = 0;
        while index < 24 {
            let (id, name) = buttons[index % 12];
            descriptors[index] = retro_input_descriptor {
                port: (index / 12) as u32,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id,
                description: name.as_ptr(),
            };
            index += 1;
        }
        descriptors
    };
    crate::callbacks::env_cmd(
        RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
        CONTROLLERS.as_ptr().cast_mut().cast(),
    );
    crate::callbacks::env_cmd(
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
        DESCRIPTORS.as_ptr().cast_mut().cast(),
    );
}
