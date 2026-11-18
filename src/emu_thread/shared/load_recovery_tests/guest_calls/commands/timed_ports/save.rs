use super::*;

fn owner(header: u8, timing: TimingModel) -> Owner {
    let backend = fixture(
        header,
        timing,
        ControllerKind::ThreeButton,
        &[
            (0x1b0, &[0x5241, 0xf820, 0x0020, 1, 0x0020, 0x3fff]),
            (4, &[0, 0x1900]),
            (0x1900, &[0x33fc, 0x8104, 0x00c0, 4, 0x4ef9, 0, 0x200]),
        ],
    );
    assert_eq!(machine(&backend).registers().pc, 0x1900);
    let owner = Owner::new(backend);
    assert_eq!(machine(&owner.backend).vdp_registers()[1], Some(4));
    assert!(machine(&owner.backend).debug_save_ports().is_some());
    owner
}

fn precise_refused(header: u8) {
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    let standard = owner(header, TimingModel::Standard);
    let rom = machine(&standard.backend).rom_bytes().to_vec();
    let path = std::path::Path::new("timed-save-precise.md");
    let result = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_timing: TimingModel::Precise,
            md_controller: ControllerKind::ThreeButton,
            apply_mods: false,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("Precise admitted an unqualified save cartridge"),
    };
    assert!(format!("{error:#}").contains("cartridge hardware is not qualified for owned timing"));
}

fn byte(address: u32, value: u8) -> DebugPortWrite {
    DebugPortWrite::Byte { address, value }
}

fn writes() -> Vec<DebugPortWrite> {
    vec![
        byte(0x200001, 0xa5),
        byte(0x200000, 0x77),
        byte(0x203fff, 0x3c),
        byte(0xa130f1, 3),
        byte(0x200001, 0x55),
        byte(0xa130f1, 1),
    ]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sram_normal_worker_matches_actual_cycles_metadata_pcm_and_next_fields() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            if timing == TimingModel::Precise {
                precise_refused(header);
                continue;
            }
            let actual = owner(header, timing);
            let mut expected = owner(header, timing);
            let registers = machine(&expected.backend).registers();
            let queue = machine(&expected.backend).queued_words();
            let fields = machine(&expected.backend).frame_count();
            let queued = machine(&expected.backend).clone().drain_audio_samples();
            assert!(queued.iter().any(|sample| *sample != 0.0));
            let direct = machine_mut(&mut expected.backend)
                .debug_write_ports(&writes())
                .unwrap();
            assert_eq!(machine(&expected.backend).peek(0x200001), Some(0xa5));
            assert_eq!(machine(&expected.backend).peek(0x200000), Some(0xff));
            assert_eq!(machine(&expected.backend).peek(0x203fff), Some(0x3c));
            assert!(
                !machine(&expected.backend)
                    .debug_save_ports()
                    .unwrap()
                    .write_protected
            );
            let mut worker = EmuThread::spawn(actual.backend, false);
            let frame = guest_call_deliver(&worker, request(940, writes()));
            let response = reply(&worker, 940);
            let result = response.result.unwrap();
            assert_eq!((result.writes, result.instructions), (6, 0));
            assert_eq!(result.cpu_clocks, direct.cpu_clocks.0);
            assert_eq!(result.master_ticks, direct.elapsed.get());
            assert_eq!(
                result.fields_crossed,
                machine(&expected.backend).frame_count() - fields
            );
            assert_eq!(response.queued_pcm_samples_before, Some(queued.len()));
            assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
            assert_eq!(machine(&expected.backend).registers(), registers);
            assert_eq!(machine(&expected.backend).queued_words(), queue);
            let metadata = frame.ui_data.md_timed_ports.unwrap().save.unwrap();
            assert_eq!(
                metadata,
                machine(&expected.backend).debug_save_ports().unwrap()
            );
            assert_eq!(
                (metadata.data_start, metadata.data_end, metadata.control),
                (0x200000, 0x203fff, 0xa130f1)
            );
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let before = machine(&expected.backend)
                .encode_debugger_checkpoint()
                .unwrap();
            let faulting = vec![byte(0x200003, 0x5a), byte(0xa130f1, 0)];
            assert!(matches!(
                machine_mut(&mut expected.backend).debug_write_ports(&faulting),
                Err(DebugPortError::Execution { index: 1, .. })
            ));
            assert_eq!(
                machine(&expected.backend)
                    .encode_debugger_checkpoint()
                    .unwrap(),
                before
            );
            let frame = guest_call_deliver(&worker, request(941, faulting));
            let failure = reply(&worker, 941).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Execution, Some(1))
            );
            assert!(frame.runtime_fault.is_none());
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let frame = guest_call_deliver(
                &worker,
                request(942, vec![byte(0x200003, 0x5a), byte(0x204001, 0)]),
            );
            let failure = reply(&worker, 942).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Port, Some(1))
            );
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let mut control_worker = EmuThread::spawn(expected.backend, false);
            for _ in 0..2 {
                let input = || {
                    let mut input = guest_call_input(1);
                    input.debug_continue = true;
                    input.debug_actions.remove_breakpoints.push(registers.pc);
                    input
                };
                let a = guest_call_deliver(&worker, input());
                let b = guest_call_deliver(&control_worker, input());
                assert!(a.runtime_fault.is_none() && b.runtime_fault.is_none());
                assert_eq!((a.advanced_frames, b.advanced_frames), (1, 1));
                assert_eq!(pcm(&a), pcm(&b));
                assert!(a.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(history(&a), history(&b));
                assert_eq!(capture(&worker), capture(&control_worker));
                assert_eq!(
                    *worker.shared_framebuffer().load_full().unwrap(),
                    *control_worker.shared_framebuffer().load_full().unwrap()
                );
            }
            worker.shutdown();
            control_worker.shutdown();
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sram_no_effect_delivery_keeps_dirty_markers_faults_rewind_and_guest_undo() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        if timing == TimingModel::Precise {
            precise_refused(b'U');
            continue;
        }
        let mut owner = owner(b'U', timing);
        let initial = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let before = session(&mut owner.backend);
        let markers = owner.markers.clone();
        let cells = machine(&owner.backend).dump_save_ram();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(943, vec![byte(0x200000, 0x77)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        assert!(response.result.is_ok() && dirty);
        assert_eq!(machine(&owner.backend).dump_save_ram(), cells);
        paused(&frame);
        let mut marked = markers;
        marked.push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
        assert_eq!(owner.markers, marked);
        let changed = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(944, vec![byte(0x200003, 0x5a), byte(0xa130f1, 0)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        let failure = response.result.unwrap_err();
        assert_eq!(
            (failure.kind, failure.index),
            (MdTimedPortFailureKind::Execution, Some(1))
        );
        assert!(!dirty && owner.fault.can_step());
        paused(&frame);
        assert_eq!(owner.markers, marked);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            changed
        );
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        let after = session(&mut owner.backend);
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.definitions, before.definitions);
        assert_eq!(after.trace, before.trace);
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            initial
        );
        owner.fault.latch(Some("prior worker fault".into()));
        let before = machine(&owner.backend).encode_state().unwrap();
        let markers = owner.markers.clone();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(945, writes()),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        assert_eq!(
            response.result.unwrap_err().kind,
            MdTimedPortFailureKind::Lifecycle
        );
        assert_eq!(frame.runtime_fault.as_deref(), Some("prior worker fault"));
        assert!(!dirty && !owner.fault.can_step());
        assert_eq!(owner.markers, markers);
        assert_eq!(machine(&owner.backend).encode_state().unwrap(), before);
    }
}
