pub(crate) fn stick_dpad_mask(left_stick: (f32, f32), deadzone: f32) -> u8 {
    let (x, y) = left_stick;
    let ax = x.abs();
    let ay = y.abs();

    let mut use_x = ax > 0.0 && ax >= deadzone;
    let mut use_y = ay > 0.0 && ay >= deadzone;

    const CARDINAL_SNAP: f32 = 0.18; // ~tan(10deg)
    if use_x && use_y {
        if ay < ax * CARDINAL_SNAP {
            use_y = false;
        } else if ax < ay * CARDINAL_SNAP {
            use_x = false;
        }
    }

    let mut mask = 0u8;
    if use_x {
        if x >= deadzone {
            mask |= 1 << 0;
        }
        if x <= -deadzone {
            mask |= 1 << 1;
        }
    }
    if use_y {
        if y >= deadzone {
            mask |= 1 << 2;
        }
        if y <= -deadzone {
            mask |= 1 << 3;
        }
    }
    mask
}

pub(crate) fn stick_dpad_vector(stick: (f32, f32), deadzone: f32) -> (f32, f32) {
    let mask = stick_dpad_mask(stick, deadzone);
    (
        u8::from(mask & 1 != 0) as f32 - u8::from(mask & 2 != 0) as f32,
        u8::from(mask & 4 != 0) as f32 - u8::from(mask & 8 != 0) as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_matches_actual_digital_mapping_at_boundaries() {
        for (stick, deadzone, expected) in [
            ((0.0, 0.0), 0.0, (0.0, 0.0)),
            ((0.249, -0.249), 0.25, (0.0, 0.0)),
            ((0.25, -0.25), 0.25, (1.0, -1.0)),
            ((1.0, 0.1), 0.05, (1.0, 0.0)),
            ((-0.1, -1.0), 0.05, (0.0, -1.0)),
            ((-0.5, 0.5), 0.25, (-1.0, 1.0)),
        ] {
            assert_eq!(stick_dpad_vector(stick, deadzone), expected);
        }
    }
}
