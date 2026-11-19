use super::*;
use crate::emu_thread::MemorySearchRequest;
use crate::emu_thread::SnapshotRequest;
use zeff_md_core::{ControllerKind, TimingModel};

const SETUP: u32 = 0x1900;
const IDLE_CODE: u32 = 0x1800;

fn factory(timing: TimingModel, sram: bool) -> EmuBackend {
    let mut words = vec![
        0x33fc, 0x0100, 0x00a1, 0x1100, 0x33fc, 0x0100, 0x00a1, 0x1200,
    ];
    words.extend([0x4e71; 4]);
    for (address, value) in [
        (0xa00042_u32, 0xd6),
        (0xa00043, 0x7b),
        (0xff1234, 0xd6),
        (0xff1235, 0x7b),
    ] {
        words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    if sram {
        for (address, value) in [(0x200041_u32, 0x5a), (0x200043, 0xa5)] {
            words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
        }
    }
    words.push(0x4e75);
    guest_call_backend_with(
        b'U',
        timing,
        ControllerKind::ThreeButton,
        &[(IDLE_CODE, &[0x7001, 0x60fe]), (SETUP, &words)],
        sram,
    )
}

fn setup(thread: &EmuThread) {
    thread.send(EmuCommand::ExecuteGuestCall(GuestCallRequest {
        name: "search cells".into(),
        target: SETUP,
        storage_offset: Some(u64::from(SETUP)),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: 32,
    }));
    assert!(matches!(
        receive_response(thread),
        EmuResponse::GuestCallCompleted { .. }
    ));
}

fn request(pattern: &[u8], max_results: usize) -> FrameInput {
    let mut request = guest_call_input(0);
    request.snapshot.memory_search = Some(MemorySearchRequest {
        pattern: pattern.to_vec(),
        max_results,
    });
    request
}

fn addresses(data: &crate::ui::UiFrameData) -> Vec<u32> {
    data.memory_search_results
        .as_ref()
        .unwrap()
        .iter()
        .map(|row| row.address)
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_known_memory_search_actual_workers_find_sound_save_and_canonical_ram_without_mutating_future()
{
    for (timing, sram) in [
        (TimingModel::Standard, false),
        (TimingModel::Precise, false),
        (TimingModel::Standard, true),
    ] {
        let mut subject = EmuThread::spawn(factory(timing, sram), false);
        let mut control = EmuThread::spawn(factory(timing, sram), false);
        let actual = step(&subject, 2, 0, 0);
        let expected = step(&control, 2, 0, 0);
        assert_eq!((actual.advanced_frames, expected.advanced_frames), (2, 2));
        assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(pcm(&actual), pcm(&expected));
        for thread in [&subject, &control] {
            let mut stop = guest_call_input(1);
            stop.debug_actions.add_breakpoint = Some(RETURN_PC);
            paused(&guest_call_deliver(thread, stop));
            setup(thread);
        }
        let before = capture(&subject);
        assert_eq!(before, capture(&control));
        let actual = guest_call_deliver(&subject, guest_call_input(0));
        let expected = guest_call_deliver(&control, guest_call_input(0));
        paused(&actual);
        paused(&expected);
        assert_eq!(pcm(&actual), pcm(&expected));
        assert!(
            actual.audio_semantic_frames.is_empty() && expected.audio_semantic_frames.is_empty()
        );
        assert_eq!(capture(&subject), before);
        assert_eq!(capture(&control), before);
        for (pattern, limit, expected) in [
            (&[0xd6, 0x7b][..], 8, vec![0xa00042, 0xff1234]),
            (&[0xd6, 0x7b][..], 1, vec![0xa00042]),
            (&[0xd6, 0x7b][..], 0, vec![]),
            (
                &[0x5a, 0xff, 0xa5][..],
                8,
                if sram { vec![0x200041] } else { vec![] },
            ),
        ] {
            let result = guest_call_deliver(&subject, request(pattern, limit));
            paused(&result);
            assert_eq!(addresses(&result.ui_data), expected);
            assert!(
                result
                    .ui_data
                    .memory_search_results
                    .as_ref()
                    .unwrap()
                    .iter()
                    .all(|row| row.matched_bytes == pattern)
            );
            assert!(result.audio_samples.is_empty());
            assert_eq!(capture(&subject), before);
        }
        if sram {
            let result = guest_call_deliver(&subject, request(&[0x5a], 16));
            paused(&result);
            assert!(addresses(&result.ui_data).contains(&0x200041));
            assert_eq!(capture(&subject), before);
        }
        for field in 0..2 {
            let next = || {
                let mut request = guest_call_input(1);
                if field == 0 {
                    request.debug_actions.remove_breakpoints.push(RETURN_PC);
                    request.debug_continue = true;
                }
                request
            };
            let actual = guest_call_deliver(&subject, next());
            let expected = guest_call_deliver(&control, next());
            assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
            assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
            assert!(expected.ui_data.memory_search_results.is_none());
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

fn inspect(backend: &mut EmuBackend, request: &SnapshotRequest) -> crate::ui::UiFrameData {
    crate::ui::collect_backend_snapshot(
        backend,
        request,
        crate::emu_thread::ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
    )
}

fn core(backend: &EmuBackend) -> &zeff_md_core::Emulator {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_known_memory_search_keeps_owned_bytes_current_overlay_and_high_source_domains_distinct() {
    for (timing, sram) in [
        (TimingModel::Standard, false),
        (TimingModel::Precise, false),
        (TimingModel::Standard, true),
    ] {
        let mut backend = factory(timing, sram);
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        let context = md
            .test_normal_core_mut()
            .unwrap()
            .begin_debug_call(SETUP)
            .unwrap();
        for _ in 0..32 {
            if md.core().debug_call_returned(&context) {
                break;
            }
            md.test_normal_core_mut()
                .unwrap()
                .step_instruction()
                .unwrap();
        }
        assert!(md.core().debug_call_returned(&context));
        md.test_normal_core_mut()
            .unwrap()
            .finish_debug_call(context)
            .unwrap();
        let patches = crate::cheats::parse_cheat_for_system("001800:7002", ActiveSystem::Md)
            .unwrap()
            .0;
        md.test_normal_core_mut()
            .unwrap()
            .install_rom_patches(&patches)
            .unwrap();
        let raw = core(&backend).encode_state().unwrap();
        let frontend = core(&backend).encode_frontend_state().unwrap();
        let owned = core(&backend).encode_debugger_checkpoint().unwrap();
        for pattern in [&[0xd6, 0x7b][..], &[0x70, 2, 0x60, 0xfe][..]] {
            let mut request = request(pattern, 8).snapshot;
            request.rom_search = Some(MemorySearchRequest {
                pattern: vec![0x70, 1, 0x60, 0xfe],
                max_results: 8,
            });
            let result = inspect(&mut backend, &request);
            assert_eq!(
                addresses(&result),
                if pattern.len() == 2 {
                    vec![0xa00042, 0xff1234]
                } else {
                    vec![0x1800]
                }
            );
            assert_eq!(
                result
                    .rom_search_results
                    .unwrap()
                    .iter()
                    .map(|row| row.offset)
                    .collect::<Vec<_>>(),
                [0x1800]
            );
            assert_eq!(core(&backend).encode_state().unwrap(), raw);
            assert_eq!(core(&backend).encode_frontend_state().unwrap(), frontend);
            assert_eq!(core(&backend).encode_debugger_checkpoint().unwrap(), owned);
            assert_eq!(core(&backend).peek(IDLE_CODE + 1), Some(2));
            assert_eq!(core(&backend).rom_bytes()[IDLE_CODE as usize + 1], 1);
        }
    }
    let mut rom = vec![0; 0x2000000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA SSF        ");
    let words = [
        0x13fc_u16, 0x00d6, 0x00ff, 0x1234, 0x13fc, 0x007b, 0x00ff, 0x1235, 0x60fe, 0x4e71,
    ];
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom[0x1000041..0x1000043].copy_from_slice(&[0xd6, 0x7b]);
    let mut emu = zeff_md_core::Emulator::new(rom, zeff_md_core::LoadConfig::default()).unwrap();
    emu.step_instruction().unwrap();
    emu.step_instruction().unwrap();
    let before = emu.encode_debugger_checkpoint().unwrap();
    let mut query = request(&[0xd6, 0x7b], 8).snapshot;
    query.rom_search = Some(MemorySearchRequest {
        pattern: vec![0xd6, 0x7b],
        max_results: 8,
    });
    let result = crate::ui::collect_md_snapshot(&emu, &query, None);
    assert_eq!(addresses(&result), [0xff1234]);
    assert_eq!(
        result
            .rom_search_results
            .unwrap()
            .iter()
            .map(|row| row.offset)
            .collect::<Vec<_>>(),
        [0x1000041]
    );
    assert_eq!(emu.peek(0x400000), None);
    let result = crate::ui::collect_md_snapshot(&emu, &request(&[0xff; 4], 8).snapshot, None);
    assert!(addresses(&result).is_empty());
    assert_eq!(emu.encode_debugger_checkpoint().unwrap(), before);
}
