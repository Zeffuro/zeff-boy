use super::*;
use crate::emu_thread::EmuCommand;
use crate::emu_thread::shared::load_recovery_tests::{capture, receive_frame, receive_response};

fn advance(thread: &EmuThread, frames: usize) {
    thread.send(EmuCommand::StepFrames(Box::new(md_audio_tests::input(
        frames, false,
    ))));
    let frame = receive_frame(thread);
    assert_eq!(frame.advanced_frames, frames);
    assert!(frame.runtime_fault.is_none() && frame.replay_error.is_none());
}

fn inspect(thread: &EmuThread, expected: &[(u32, u8)]) {
    let mut restored = fixture(b'U', TimingModel::Standard);
    restored.load_state_from_bytes(capture(thread)).unwrap();
    for &(address, value) in expected {
        assert_eq!(byte(&restored, address), value, "{address:06X}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_database_actual_platform_worker_import_enable_reject_and_disable() {
    let mut thread = EmuThread::spawn(fixture(b'U', TimingModel::Standard), false);
    let before = capture(&thread);
    let mut state = CheatState::new();
    state.active_system = ActiveSystem::Md;
    state.libretro_platform = crate::libretro_common::LibretroPlatform::Md;
    state.cheats_dirty = false;
    let (tx, rx) = std::sync::mpsc::channel();
    state.libretro_rx = Some(rx);
    state.libretro_busy = true;
    tx.send(LibretroAsyncResult::Downloaded {
        filename: "Synthetic transport.cht".into(),
        result: Ok(concat!(
            "cheat0_desc = \"Group\"\n",
            "cheat0_code = \"AABT-AN3W+0114D2:4E75+011504:4E75\"\n",
            "cheat0_enable = false\n",
            "cheat1_code = \"FF0100:A5\"\ncheat1_enable = false\n",
        )
        .into()),
    })
    .unwrap();
    drop(tx);
    poll_libretro_results_for_test(&mut state);
    assert!(state.cheats_dirty && !state.libretro_busy && state.libretro_rx.is_none());
    assert_eq!(state.libretro_codes.len(), 2);
    assert_eq!(state.libretro_codes[0].name, "Group");
    assert!(state.libretro_codes.iter().all(|code| !code.enabled));
    assert_eq!(
        state.libretro_status.as_deref(),
        Some("Imported 2 cheat(s) from Synthetic transport.cht")
    );
    thread.send(EmuCommand::UpdateCheats(collect_enabled_patches(
        &state.user_codes,
        &state.libretro_codes,
    )));
    assert_eq!(capture(&thread), before);
    for code in &mut state.libretro_codes {
        code.enabled = true;
    }
    let restored =
        parse_cht_file_for_system(&export_cht_file(&state.libretro_codes), ActiveSystem::Md);
    let patches = collect_enabled_patches(&state.user_codes, &restored);
    assert_eq!(
        patches,
        codes("000332:6600+0114D2:4E75+011504:4E75+FF0100:A5")
    );
    thread.send(EmuCommand::UpdateCheats(patches));
    advance(&thread, 2);
    let enabled = [
        (0xff0040, 0x66),
        (0xff0041, 0),
        (0xff0042, 0x4e),
        (0xff0043, 0x75),
        (0xff0044, 0x4e),
        (0xff0045, 0x75),
        (0xff0101, 0xa5),
    ];
    inspect(&thread, &enabled);
    let before_rejection = capture(&thread);
    thread.send(EmuCommand::UpdateCheats(codes("020000:70+FF0100:11")));
    let EmuResponse::CheatsUpdateFailed(message) = receive_response(&thread) else {
        panic!("missing cheat rejection");
    };
    assert!(message.contains("outside the source image"));
    assert_eq!(capture(&thread), before_rejection);
    thread.send(EmuCommand::Reset);
    advance(&thread, 2);
    inspect(&thread, &enabled);
    for code in &mut state.libretro_codes {
        code.enabled = false;
    }
    let disabled = collect_enabled_patches(&state.user_codes, &state.libretro_codes);
    assert!(disabled.is_empty());
    thread.send(EmuCommand::UpdateCheats(disabled));
    thread.send(EmuCommand::Reset);
    advance(&thread, 2);
    inspect(
        &thread,
        &[
            (0xff0040, 0),
            (0xff0041, 0),
            (0xff0042, 0),
            (0xff0043, 0),
            (0xff0044, 0),
            (0xff0045, 0),
        ],
    );
    thread.shutdown();
}
