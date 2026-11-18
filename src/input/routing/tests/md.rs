use super::*;
use crate::netplay::md_input::project_local_host;
use zeff_md_core::ControllerKind;

#[test]
fn md_actual_gamepad_routes_preserve_all_twelve_sources_before_local_projection() {
    let settings = crate::settings::Settings::default();
    let mut router = typed_router(&settings, 1);
    let id = RuntimeGamepadId(1);
    let rows = [
        ("West", 0x0040),
        ("South", 0x0001),
        ("East", 0x0002),
        ("Start", 0x0008),
        ("Select", 0x0004),
        ("LeftTrigger", 0x0010),
        ("North", 0x0080),
        ("RightTrigger", 0x0020),
        ("DPadRight", 0x0100),
        ("DPadLeft", 0x0200),
        ("DPadUp", 0x0400),
        ("DPadDown", 0x0800),
    ];
    for (name, expected) in rows {
        router.button(id, name, true);
        let actual = resolve_typed(&mut router, &settings, false).players[0].buttons;
        let buttons = (actual >> 4) as u8;
        let dpad = (actual & 0x0f) as u8;
        assert_eq!(u16::from(buttons) | u16::from(dpad) << 8, expected);
        assert_eq!(
            project_local_host(ControllerKind::ThreeButton, buttons, dpad).unwrap(),
            expected & 0x0f4b
        );
        assert_eq!(
            project_local_host(ControllerKind::SixButton, buttons, dpad).unwrap(),
            expected
        );
        router.button(id, name, false);
        resolve_typed(&mut router, &settings, false);
    }
    for (name, _) in rows {
        router.button(id, name, true);
    }
    let actual = resolve_typed(&mut router, &settings, false).players[0].buttons;
    assert_eq!(actual, 0x0fff);
    assert_eq!(
        project_local_host(
            ControllerKind::ThreeButton,
            (actual >> 4) as u8,
            (actual & 0x0f) as u8
        )
        .unwrap(),
        0x0f4b
    );
    assert_eq!(
        project_local_host(
            ControllerKind::SixButton,
            (actual >> 4) as u8,
            (actual & 0x0f) as u8
        )
        .unwrap(),
        0x0fff
    );
}
