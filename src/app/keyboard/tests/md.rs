use super::*;
use crate::netplay::md_input::project_local_host;
use zeff_md_core::ControllerKind;

#[test]
fn md_actual_keyboard_bindings_preserve_generic_x_and_project_only_absent_buttons() {
    let directory = crate::test_support::test_directory("md-keyboard").unwrap();
    let path = directory.path().join("input.md");
    std::fs::write(&path, crate::netplay::md_fixture::rom()).unwrap();
    let backend = crate::emu_backend::load_backend_from_rom_source(
        ActiveSystem::Md,
        &path,
        &path,
        None,
        crate::netplay::md_fixture::config(
            zeff_md_core::Region::NtscUs,
            zeff_md_core::TimingModel::Standard,
            ControllerKind::ThreeButton,
        ),
    )
    .unwrap()
    .backend;
    let mut app = app_with_worker(EmuThread::spawn(backend, false), 1, ActiveSystem::Md, path);
    let rows = [
        (BindingAction::X, KeyCode::F1, 0x0040),
        (BindingAction::A, KeyCode::F2, 0x0001),
        (BindingAction::B, KeyCode::F3, 0x0002),
        (BindingAction::Start, KeyCode::F4, 0x0008),
        (BindingAction::Select, KeyCode::F5, 0x0004),
        (BindingAction::L, KeyCode::F6, 0x0010),
        (BindingAction::Y, KeyCode::F7, 0x0080),
        (BindingAction::R, KeyCode::F8, 0x0020),
        (BindingAction::Right, KeyCode::F9, 0x0100),
        (BindingAction::Left, KeyCode::F10, 0x0200),
        (BindingAction::Up, KeyCode::F11, 0x0400),
        (BindingAction::Down, KeyCode::F12, 0x0800),
    ];
    for (action, key, _) in rows {
        app.settings
            .set_binding_set(
                &InputScope::Global,
                BindingTarget::Joypad { player: 1, action },
                GameplayBindingSource::Keyboard,
                Some(BindingSet::new(BindingExpression::keyboard(key))),
            )
            .unwrap();
    }
    app.sync_effective_input();
    for (_, key, word) in rows {
        admit_key(&mut app, key);
        let (buttons, dpad) = app.current_host_joypad_input();
        assert_eq!(u16::from(buttons) | u16::from(dpad) << 8, word);
        assert_eq!(
            project_local_host(ControllerKind::ThreeButton, buttons, dpad).unwrap(),
            word & 0x0f4b
        );
        assert_eq!(
            project_local_host(ControllerKind::SixButton, buttons, dpad).unwrap(),
            word
        );
        release_key(&mut app, key);
    }
    for (_, key, _) in rows {
        admit_key(&mut app, key);
    }
    let (buttons, dpad) = app.current_host_joypad_input();
    assert_eq!((buttons, dpad), (0xff, 0x0f));
    assert_eq!(
        project_local_host(ControllerKind::ThreeButton, buttons, dpad).unwrap(),
        0x0f4b
    );
    assert_eq!(
        project_local_host(ControllerKind::SixButton, buttons, dpad).unwrap(),
        0x0fff
    );
    app.stop_emu_thread();
}
