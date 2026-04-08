use crate::api::*;
use crate::callbacks::{CB_INPUT_STATE, lock};
use std::os::raw::c_uint;

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

pub(crate) fn set_input_descriptors(is_nes: bool) {
    use crate::callbacks::env_cmd;
    use std::os::raw::c_void;

    let mut descs: Vec<retro_input_descriptor> = vec![
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: c"A".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_B,
            description: c"B".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_SELECT,
            description: c"Select".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_START,
            description: c"Start".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_UP,
            description: c"D-Pad Up".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_DOWN,
            description: c"D-Pad Down".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_LEFT,
            description: c"D-Pad Left".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_RIGHT,
            description: c"D-Pad Right".as_ptr(),
        },
    ];

    if is_nes {
        descs.extend_from_slice(&[
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_A,
                description: c"A".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_B,
                description: c"B".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_SELECT,
                description: c"Select".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_START,
                description: c"Start".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_UP,
                description: c"D-Pad Up".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_DOWN,
                description: c"D-Pad Down".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_LEFT,
                description: c"D-Pad Left".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_RIGHT,
                description: c"D-Pad Right".as_ptr(),
            },
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
