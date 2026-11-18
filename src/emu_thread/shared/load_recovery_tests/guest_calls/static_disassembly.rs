use super::*;
use crate::debug::{CpuDisassemblyTarget, DisassemblyCursor, DisassemblyTarget};
use zeff_md_core::{ControllerKind, TimingModel};

fn factory(timing: TimingModel) -> EmuBackend {
    guest_call_backend_with(
        b'U',
        timing,
        ControllerKind::ThreeButton,
        &[(0x1800, &[0x7001, 0x60fe])],
        false,
    )
}

fn patches() -> Vec<crate::cheats::CheatPatch> {
    crate::cheats::parse_cheat_for_system("001800:7002", ActiveSystem::Md)
        .unwrap()
        .0
}

fn inspect_request(target: DisassemblyTarget) -> FrameInput {
    let mut request = guest_call_input(0);
    request.snapshot.show_disassembler = true;
    request.snapshot.disasm_target = Some(target);
    request
}

fn source_view(view: &crate::debug::DisassemblyView) {
    assert_eq!(view.cursor, DisassemblyCursor::RomStorage(0x1800));
    assert!(view.is_static_target && view.is_navigation_target);
    assert!(view.breakpoints.is_empty() && view.one_shot_breakpoints.is_empty());
    assert_eq!(view.lines[0].storage_offset, Some(0x1800));
    assert_eq!(view.lines[0].bytes.as_slice(), [0x70, 1]);
    assert_eq!(view.lines[0].mnemonic.as_str(), "MOVEQ #1,D0");
    assert!(view.lines.iter().all(|line| line.control_target.is_none()
        && line.control_target_storage.is_none()
        && line.bank.is_none()));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_static_rom_actual_workers_inspect_original_bytes_and_preserve_future_fields() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut subject = EmuThread::spawn(factory(timing), false);
        let mut control = EmuThread::spawn(factory(timing), false);
        for thread in [&subject, &control] {
            thread.send(EmuCommand::UpdateCheats(patches()));
        }
        let actual = step(&subject, 2, 0, 0);
        let expected = step(&control, 2, 0, 0);
        assert_eq!((actual.advanced_frames, expected.advanced_frames), (2, 2));
        assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert!(
            actual.ui_data.disassembly_view.is_none()
                && expected.ui_data.disassembly_view.is_none()
        );
        assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(pcm(&actual), pcm(&expected));
        let stop = || {
            let mut input = guest_call_input(1);
            input.debug_actions.add_breakpoint = Some(RETURN_PC);
            input
        };
        paused(&guest_call_deliver(&subject, stop()));
        paused(&guest_call_deliver(&control, stop()));
        let before = capture(&subject);
        assert_eq!(before, capture(&control));
        for _ in 0..3 {
            let result = guest_call_deliver(
                &subject,
                inspect_request(DisassemblyTarget::RomStorage(0x1800)),
            );
            paused(&result);
            assert!(result.audio_samples.is_empty() && result.audio_semantic_frames.is_empty());
            source_view(result.ui_data.disassembly_view.as_ref().unwrap());
            assert_eq!(capture(&subject), before);
        }
        let live = guest_call_deliver(
            &subject,
            inspect_request(DisassemblyTarget::Cpu(CpuDisassemblyTarget::m68000(
                0x8000_1800,
            ))),
        );
        paused(&live);
        assert_eq!(
            live.ui_data.disassembly_view.as_ref().unwrap().cursor,
            DisassemblyCursor::Cpu(0x1800)
        );
        assert_eq!(
            live.ui_data.disassembly_view.unwrap().lines[0]
                .bytes
                .as_slice(),
            [0x70, 2]
        );
        assert_eq!(capture(&subject), before);
        for field in 0..2 {
            let request = || {
                let mut input = guest_call_input(1);
                if field == 0 {
                    input.debug_actions.remove_breakpoints.push(RETURN_PC);
                    input.debug_continue = true;
                }
                input
            };
            let actual = guest_call_deliver(&subject, request());
            let expected = guest_call_deliver(&control, request());
            assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
            assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
            assert!(expected.ui_data.disassembly_view.is_none());
            assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm(&actual), pcm(&expected));
            assert_eq!(capture(&subject), capture(&control));
            assert!(!subject.shared_framebuffer().load_full().unwrap().is_empty());
            assert_eq!(
                subject.shared_framebuffer().load_full(),
                control.shared_framebuffer().load_full()
            );
        }
        subject.shutdown();
        control.shutdown();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_static_rom_backend_snapshots_preserve_frontend_owned_queue_and_current_overlay() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = factory(timing);
        let mut current = Vec::new();
        assert!(
            super::super::super::md_rom_cheat_tests::update(&mut backend, &mut current, patches())
                .is_none()
        );
        let initial_frontend = machine(&backend).encode_frontend_state().unwrap();
        for imported in [false, true] {
            if imported {
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                md.test_normal_core_mut()
                    .unwrap()
                    .decode_frontend_state(&initial_frontend)
                    .unwrap();
            }
            let before = machine(&backend);
            let raw = before.encode_state().unwrap();
            let frontend = before.encode_frontend_state().unwrap();
            let owned = before.encode_debugger_checkpoint().unwrap();
            for _ in 0..3 {
                let request = inspect_request(DisassemblyTarget::RomStorage(0x1800));
                let data = crate::ui::collect_backend_snapshot(
                    &mut backend,
                    &request.snapshot,
                    crate::emu_thread::ReusableBuffers {
                        audio: None,
                        vram: None,
                        oam: None,
                        memory_page: None,
                        nes_chr: None,
                        nes_nametable: None,
                    },
                );
                source_view(data.disassembly_view.as_ref().unwrap());
                let actual = machine(&backend);
                assert_eq!(actual, before);
                assert_eq!(actual.encode_state().unwrap(), raw);
                assert_eq!(actual.encode_frontend_state().unwrap(), frontend);
                assert_eq!(actual.encode_debugger_checkpoint().unwrap(), owned);
                assert_eq!(actual.peek(0x1801), Some(2));
                assert_eq!(actual.rom_bytes()[0x1801], 1);
            }
        }
    }
}
