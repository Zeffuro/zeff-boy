use super::*;
use crate::cartridge::sf001::Revision;
use sha2::{Digest, Sha256};
#[cfg(feature = "first-failure-diagnostic")]
use zeff_m68k::bus::{AccessKind, FunctionCode};
use zeff_m68k::bus::{ByteLanes, Direction};

#[cfg(feature = "first-failure-diagnostic")]
mod watch;
#[cfg(feature = "first-failure-diagnostic")]
mod write_watch;

fn fixture(concurrent: bool, fault: bool, odd: bool, sound: u8, dma: bool) -> Emulator {
    fixture_program(concurrent, fault, odd, sound, dma, false)
}

fn fixture_program(
    concurrent: bool,
    fault: bool,
    odd: bool,
    sound: u8,
    dma: bool,
    delayed_width: bool,
) -> Emulator {
    let mut rom = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    let pc = if fault { 0x3ffe22 } else { 0x200 };
    rom[4..8].copy_from_slice(&(pc as u32).to_be_bytes());
    rom[27 * 4..27 * 4 + 4].copy_from_slice(&0x300_u32.to_be_bytes());
    for (index, word) in [0x4e70_u16, 0x4ad0, 0x1039, 0x0040, 0x0000]
        .iter()
        .enumerate()
    {
        rom[0x300 + index * 2..0x302 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let program: &[u16] = if fault {
        &[0x12d8, 0x51c8, 0xfffc, 0x4e72, 0x2700]
    } else {
        &[0x60fe, 0x4e71]
    };
    for (index, word) in program.iter().enumerate() {
        rom[pc + index * 2..pc + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
    if delayed_width {
        for index in 0..512 {
            rom[pc + index * 2..pc + index * 2 + 2].copy_from_slice(&0x4e71_u16.to_be_bytes());
        }
        for (index, word) in [0x33fc_u16, 0x8c81, 0x00c0, 0x0004, 0x60fe]
            .iter()
            .enumerate()
        {
            let at = pc + (512 + index) * 2;
            rom[at..at + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    rom[0x3ffffe..].fill(0);
    let sum = rom[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    rom[0x3ffffe..].copy_from_slice(&0x3e08_u16.wrapping_sub(sum).to_be_bytes());
    if !fault {
        rom[0x180..0x18e].fill(0);
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    assert_eq!(machine.bus.cartridge.sf001.is_some(), fault);
    if fault {
        // Keep the failure outside the unlocked board's open-bus path.
        let board = machine.bus.cartridge.sf001.as_mut().unwrap();
        assert!(board.control(0x0e00, 0x20));
        assert!(board.locked());
    }
    machine.cpu.registers.a[0] = if odd {
        0x400001
    } else if dma {
        0x400000
    } else {
        0x3ffffe
    };
    machine.cpu.registers.a[1] = if odd || dma { 0xff01d2 } else { 0xff01d0 };
    machine.cpu.registers.d[0] = if odd || dma { 0xffc00200 } else { 0xffc00202 };
    if sound != 0 {
        machine.bus.sound.set_reset(false);
        let program: &[u8] = if sound == 1 {
            &[0x00, 0x00, 0x76]
        } else {
            &[0x21, 0x00, 0x80, 0x7e, 0x76]
        };
        machine.bus.sound.bank = 0x1ff;
        machine.bus.sound.ram[..program.len()].copy_from_slice(program);
    }
    if dma {
        for (index, value) in [
            (1, 0x54),
            (19, 17),
            (20, 0),
            (21, 0),
            (22, 0xc0),
            (23, 0x7f),
        ] {
            machine.bus.advance_machine(1).unwrap();
            let write = machine
                .bus
                .vdp
                .queue_register(index, value, Region::NtscUs)
                .unwrap();
            machine
                .bus
                .advance_machine(write.publish_at.get() - machine.bus.now.get())
                .unwrap();
        }
        machine
            .bus
            .vdp
            .write_control(0x4400, Region::NtscUs)
            .unwrap();
        machine.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
    }
    machine
}

fn compare(actual: &mut Emulator, control: &mut Emulator, hash: &mut Sha256) {
    assert_eq!(actual, control);
    assert_eq!(actual.timing_snapshot(), control.timing_snapshot());
    assert_eq!(actual.framebuffer(), control.framebuffer());
    assert_eq!(
        actual.framebuffer_dimensions(),
        control.framebuffer_dimensions()
    );
    assert_eq!(actual.video_error(), control.video_error());
    let actual_raw = actual.encode_state().map_err(|error| error.to_string());
    let control_raw = control.encode_state().map_err(|error| error.to_string());
    assert_eq!(actual_raw, control_raw);
    assert_eq!(
        actual
            .encode_frontend_state()
            .map_err(|error| error.to_string()),
        control
            .encode_frontend_state()
            .map_err(|error| error.to_string()),
    );
    match actual_raw {
        Ok(raw) => hash.update(&raw),
        Err(error) => hash.update(error.as_bytes()),
    }
    hash.update(format!("{:?}{:?}", actual.registers(), actual.queued_words()).as_bytes());
    hash.update(actual.work_ram());
    hash.update(actual.cpu_clock().0.to_le_bytes());
    let actual_audio: Vec<_> = actual
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    let control_audio: Vec<_> = control
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    assert_eq!(actual_audio, control_audio);
    for value in actual_audio {
        hash.update(value.to_le_bytes());
    }
    assert_eq!(actual.take_vdp_capture(), control.take_vdp_capture());
    assert_eq!(actual, control);
}

#[cfg(feature = "first-failure-diagnostic")]
fn assert_fault_report(
    report: &crate::diagnostic::Report,
    machine: &Emulator,
    odd: bool,
    sound: u8,
    dma: bool,
) {
    use crate::diagnostic::{BoardLocation, Callback, Observation, Outcome};
    assert!(report.complete);
    assert_eq!(report.terminal, machine.diagnostic_snapshot());
    let entry = report.entry.unwrap();
    assert_eq!(entry.snapshot.registers.pc, 0x3ffe22);
    assert_eq!(entry.snapshot.registers.a[0], 0x400000 + u32::from(odd));
    assert_eq!(entry.snapshot.registers.a[1], 0xff01d2);
    assert_eq!(entry.snapshot.registers.d[0], 0xffc00200);
    assert_eq!(entry.snapshot.queued_words, Some([0x12d8, 0x51c8]));
    assert_eq!(machine.registers().a[0], 0x400001 + u32::from(odd));
    assert_eq!(machine.work_ram()[0x1d2], 0);
    let records: Vec<_> = report.history.iter().flatten().collect();
    let failed = records.last().unwrap();
    assert_eq!(failed.callback, Callback::Read);
    assert_eq!(report.first_failed_sequence, Some(failed.sequence));
    assert_eq!(failed.access.unwrap().kind, AccessKind::Data);
    assert_eq!(
        failed.access.unwrap().function_code,
        FunctionCode::SupervisorData
    );
    assert_eq!(
        failed.access.unwrap().logical_address,
        0x400000 + u32::from(odd)
    );
    assert_eq!(
        failed.outcome,
        Some(Outcome::UnsupportedAccess {
            address: 0x400000,
            direction: Direction::Read,
        })
    );
    let service = failed.service.unwrap();
    assert_eq!(
        service.board.unwrap().target,
        Some(BoardLocation::Unclaimed)
    );
    assert!(service.master_ticks > failed.entry.bus.master_ticks);
    assert_eq!(failed.returned.unwrap().bus.prefetch, Observation::Empty);
    if sound == 0 && !dma {
        assert_eq!(service.prefetch, Observation::Full(0x51c8));
    } else if machine.concurrent_sound || dma {
        assert_eq!(service.prefetch, Observation::Empty);
    }
    if dma {
        assert!(failed.wait_clocks > 0);
        assert!(service.dma_phase_and_release_unavailable);
    }
    if !odd && !dma {
        assert!(records.windows(2).any(|pair| {
            pair[0]
                .access
                .is_some_and(|access| access.bus_address == 0x3ffe22)
                && pair[0].outcome == Some(Outcome::Read(0x12d8))
                && pair[1]
                    .access
                    .is_some_and(|access| access.bus_address == 0x3ffe24)
                && pair[1].outcome == Some(Outcome::Read(0x51c8))
        }));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_actual_field_and_first_failure_contract() {
    #[cfg(feature = "first-failure-diagnostic")]
    watch::ordinary_fields_preserve_watch_and_machine();
    #[cfg(feature = "first-failure-diagnostic")]
    write_watch::ordinary_fields_preserve_completed_writes();
    #[cfg(feature = "first-failure-diagnostic")]
    pending_sound_observation_is_pure();
    #[cfg(feature = "first-failure-diagnostic")]
    failed_sound_preparation_is_pure();
    let mut hash = Sha256::new();
    for concurrent in [false, true] {
        for (fault, odd, sound, dma) in [
            (false, false, 0, false),
            (false, false, 1, false),
            (false, false, 2, false),
            (true, false, 0, false),
            (true, true, 0, false),
            (true, false, 1, false),
            (true, false, 2, false),
            (true, false, 0, true),
        ] {
            let mut actual = fixture(concurrent, fault, odd, sound, dma);
            let mut control = actual.clone();
            for _ in 0..if fault { 1 } else { 2 } {
                let expected = control.step_field();
                #[cfg(feature = "first-failure-diagnostic")]
                let capture = crate::diagnostic::Capture::begin().unwrap();
                let result = actual.step_field();
                #[cfg(feature = "first-failure-diagnostic")]
                let report = capture.finish(&actual).unwrap();
                assert_eq!(result, expected);
                if fault {
                    assert!(matches!(result, Err(MachineFault::Main(TimingFault::Bus {
                        access,
                        direction: Direction::Read,
                        source: BusError::UnsupportedAccess { address: 0x400000, direction: Direction::Read },
                    })) if access.lanes == if odd { ByteLanes::Lower } else { ByteLanes::Upper }));
                    #[cfg(feature = "first-failure-diagnostic")]
                    assert_fault_report(&report, &actual, odd, sound, dma);
                } else {
                    result.unwrap();
                    #[cfg(feature = "first-failure-diagnostic")]
                    {
                        assert!(report.complete);
                        assert!(report.total_callbacks > 64);
                        assert_eq!(report.overwritten, report.total_callbacks - 64);
                        assert!(
                            report
                                .history
                                .iter()
                                .flatten()
                                .all(|record| record.outcome.is_some())
                        );
                    }
                }
                hash.update([
                    u8::from(concurrent),
                    u8::from(fault),
                    u8::from(odd),
                    sound,
                    u8::from(dma),
                ]);
                compare(&mut actual, &mut control, &mut hash);
            }
        }
    }
    delayed_width_field_contract(&mut hash);
    let digest = hash.finalize();
    let receipt: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    #[cfg(not(target_arch = "wasm32"))]
    println!("diagnostic-field-contract-v1 {receipt}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("diagnostic-field-contract-v1 {}", receipt);
}

fn delayed_width_field_contract(hash: &mut Sha256) {
    for concurrent in [false, true] {
        let mut actual = fixture_program(concurrent, false, false, 0, false, true);
        let mut control = actual.clone();
        let expected: Vec<_> = (0..2)
            .map(|_| {
                let result = control.step_field();
                let endpoint = control.clone();
                let _ = control.drain_audio_samples();
                let _ = control.take_vdp_capture();
                (result, endpoint)
            })
            .collect();
        #[cfg(feature = "first-failure-diagnostic")]
        let capture = crate::diagnostic::field::Capture::begin().unwrap();
        #[cfg(feature = "first-failure-diagnostic")]
        capture.after_load(&actual);
        #[cfg(feature = "first-failure-diagnostic")]
        {
            assert!(matches!(
                crate::diagnostic::Capture::begin(),
                Err(crate::diagnostic::CaptureError::AlreadyActive)
            ));
            assert!(matches!(
                crate::diagnostic::field::Capture::begin(),
                Err(crate::diagnostic::field::CaptureError::AlreadyActive)
            ));
        }
        for (result, mut endpoint) in expected {
            assert_eq!(actual.step_field(), result);
            result.unwrap();
            hash.update([u8::from(concurrent), 0x8c]);
            compare(&mut actual, &mut endpoint, hash);
        }
        #[cfg(feature = "first-failure-diagnostic")]
        {
            let report = capture.finish(&actual).unwrap();
            assert_delayed_width_report(&report, &actual);
            field_failure_cleanup(concurrent);
        }
    }
}

#[cfg(feature = "first-failure-diagnostic")]
fn assert_delayed_width_report(report: &crate::diagnostic::field::Report, actual: &Emulator) {
    use crate::diagnostic::field::{Kind, Label, RowReason, cause};
    assert!(report.complete, "{report:#?}");
    assert_eq!(
        (report.issues, report.observer_dropped, report.skip_count),
        (0, 0, 0)
    );
    assert_eq!(
        (
            report.calls,
            report.successful_calls,
            report.final_boundaries
        ),
        (2, 2, 2)
    );
    assert_eq!(
        report.terminal,
        crate::diagnostic::field::Endpoint::read(actual)
    );
    let events: Vec<_> = report.events.iter().flatten().collect();
    let labels: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            Kind::Endpoint { label, .. } => Some(*label),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels,
        [
            Label::AfterLoad,
            Label::BeforeCall1,
            Label::AfterCall1,
            Label::BeforeCall2,
            Label::AfterCall2
        ]
    );
    let publication = events.iter().position(|event| matches!(&event.kind,
        Kind::Publication { before, after } if before.vdp.pending.is_some_and(|write|
            write.index == 12 && write.value == 0x81 && write.publish_at <= after.vdp.register_ticks)
    )).unwrap();
    assert!(
        events[..publication]
            .iter()
            .any(|event| matches!(&event.kind,
                Kind::RowRun(run) if run.reason == RowReason::Accepted
                    && run.after.is_some_and(|state| state.width == 256 && state.rows > 1)
            ))
    );
    let reset = events.iter().position(|event| matches!(&event.kind,
        Kind::RowTransition { context, before, after, causes } if causes & cause::DIMENSIONS != 0
            && context.row > 0 && before.rows > 0 && before.width == 256
            && after.width == 320 && after.rows == 0 && causes & cause::ROWS_MISMATCH != 0
    )).unwrap();
    assert!(publication < reset);
    assert!(events[reset + 1..].iter().any(|event| event.call == 1
        && matches!(&event.kind,
            Kind::RowRun(run) if run.reason == RowReason::RowsMismatch && run.count > 1
        )));
    assert!(!events.iter().any(|event| event.call == 1
        && matches!(&event.kind,
            Kind::RowTransition { causes, .. } if causes & cause::PUBLISHED != 0
        )));
    let recovery = events.iter().position(|event| event.call == 2 && matches!(&event.kind,
        Kind::RowTransition { context, after, causes, .. } if context.row == 0
            && causes & (cause::ROW_ZERO | cause::ACCEPTED) == (cause::ROW_ZERO | cause::ACCEPTED)
            && after.rows == 1
    )).unwrap();
    let published = events.iter().position(|event| event.call == 2 && matches!(&event.kind,
        Kind::RowTransition { context, after, causes, .. } if causes & cause::PUBLISHED != 0
            && after.rows == context.height && after.presented == context.completed_field.id + 1 && after.ready
    )).unwrap();
    assert!(recovery < published);
    assert!(
        events[recovery + 1..published]
            .iter()
            .any(|event| matches!(&event.kind,
                Kind::RowRun(run) if run.reason == RowReason::Accepted && run.count > 1
                    && run.before.is_some_and(|state| state.rows == 1)
                    && run.after.is_some_and(|state| state.rows + 1 == run.first.height)
            ))
    );
    let counts: Vec<_> = report.fields.iter().flatten().collect();
    assert_eq!(counts.len(), 2);
    assert!(counts[0].accepted > 1 && counts[0].mismatch > 1);
    assert_eq!(counts[1].accepted, 224);
    assert_eq!(counts[1].mismatch, 0);
    for counts in counts {
        assert_eq!(
            counts.offered,
            counts.accepted + counts.render_error + counts.mismatch + counts.outside_height
        );
    }
    for call in [1, 2] {
        let crossed: Vec<_> = events
            .iter()
            .filter_map(|event| match &event.kind {
                Kind::Calendar {
                    before,
                    after,
                    crossed_at,
                } if event.call == call => {
                    assert_eq!(before.field.unwrap().id + 1, after.field.unwrap().id);
                    assert_eq!(*crossed_at, after.field.unwrap().start);
                    Some(*crossed_at)
                }
                _ => None,
            })
            .collect();
        assert_eq!(crossed.len(), 1);
        let (after, pacing_after) = events
            .iter()
            .find_map(|event| match &event.kind {
                Kind::Boundary {
                    before,
                    after,
                    pacing_before,
                    pacing_after,
                    result,
                } if event.call == call => {
                    assert!(result.is_ok());
                    assert_eq!(*pacing_before + 1, *pacing_after);
                    assert!(before.context.bus.master_ticks <= crossed[0]);
                    assert!(crossed[0] <= after.context.bus.master_ticks);
                    Some((*after, *pacing_after))
                }
                _ => None,
            })
            .unwrap();
        let endpoint = events
            .iter()
            .find_map(|event| match &event.kind {
                Kind::Endpoint {
                    label: Label::AfterCall1 | Label::AfterCall2,
                    endpoint,
                    result,
                } if event.call == call => {
                    assert_eq!(result, &Some(Ok(())));
                    Some(endpoint)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(endpoint.machine.registers, after.registers);
        assert_eq!(endpoint.machine.queued_words, after.queued_words);
        assert_eq!(endpoint.machine.cpu_clock, after.cpu_clock);
        assert_eq!(endpoint.machine.context.bus, after.context.bus);
        assert_eq!(endpoint.pacing, pacing_after);
        assert_eq!(endpoint.raster.field.unwrap().id, pacing_after);
    }
}

#[cfg(feature = "first-failure-diagnostic")]
fn field_failure_cleanup(concurrent: bool) {
    use crate::diagnostic::field::{Capture, Kind, issue};
    let mut actual = fixture(concurrent, true, false, 0, false);
    let mut control = actual.clone();
    let expected = control.step_field();
    let capture = Capture::begin().unwrap();
    capture.after_load(&actual);
    let result = actual.step_field();
    assert_eq!(result, expected);
    assert!(result.is_err());
    let report = capture.finish(&actual).unwrap();
    assert!(!report.complete);
    assert_eq!(report.issues, issue::CALLS);
    assert_eq!(report.observer_dropped, 0);
    assert!(
        report
            .events
            .iter()
            .flatten()
            .any(|event| matches!(&event.kind,
                Kind::Boundary { result: Err(fault), .. } if Some(fault) == result.as_ref().err()
            ))
    );
    assert_eq!(actual, control);
    let capture = Capture::begin().unwrap();
    drop(capture);
    drop(Capture::begin().unwrap());
    crate::diagnostic::field::assert_loss_invalidates(&actual);
}

#[cfg(feature = "first-failure-diagnostic")]
fn pending_sound_observation_is_pure() {
    for (opcode, fetch) in [(0x18, 4), (0x10, 5)] {
        let mut machine = fixture(true, false, false, 1, false);
        machine.bus.sound.ram[..2].copy_from_slice(&[opcode, 0xfe]);
        let mut phases = 0;
        for _ in 0..24 {
            if let Some(observation) = machine.diagnostic_pending_sound() {
                assert_eq!(observation.instruction_opcode, opcode);
                assert!(observation.segment_cycles == fetch || observation.segment_cycles == 8);
                let phase = if observation.segment_cycles == fetch {
                    1
                } else {
                    2
                };
                if phases & phase != 0 {
                    machine.step_instruction().unwrap();
                    continue;
                }
                phases |= phase;
                assert_eq!(observation.unclocked_bytes[..2], [Some(opcode), Some(0xfe)]);
                assert_eq!(
                    observation.deadline - observation.start,
                    u64::from(observation.segment_cycles) * 15
                );
                let before = machine.clone();
                let raw = machine.encode_state().unwrap();
                let capture = crate::diagnostic::Capture::begin().unwrap();
                for _ in 0..3 {
                    assert_eq!(machine.diagnostic_pending_sound(), Some(observation));
                }
                let report = capture.finish(&machine).unwrap();
                assert!(report.complete);
                assert_eq!(report.total_callbacks, 0);
                assert_eq!(report.schema, 2);
                assert_eq!(machine, before);
                assert_eq!(machine.encode_state().unwrap(), raw);
                let front = machine.encode_frontend_state().unwrap();
                for frontend in [false, true] {
                    let mut control = machine.clone();
                    let mut restored = machine.clone();
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                    } else {
                        restored.decode_state(&raw).unwrap();
                    }
                    let mut hash = Sha256::new();
                    for _ in 0..16 {
                        assert_eq!(restored.step_instruction(), control.step_instruction());
                        assert_eq!(
                            restored.encode_state().unwrap(),
                            control.encode_state().unwrap()
                        );
                        assert_eq!(restored.cpu, control.cpu);
                        assert_eq!(restored.sound_cpu, control.sound_cpu);
                        assert_eq!(restored.bus.prefetch, control.bus.prefetch);
                        assert_eq!(restored.cpu_clock(), control.cpu_clock());
                        assert_eq!(restored.timing_snapshot(), control.timing_snapshot());
                        assert_eq!(restored.framebuffer(), control.framebuffer());
                        assert_eq!(
                            restored.framebuffer_dimensions(),
                            control.framebuffer_dimensions()
                        );
                        assert_eq!(restored.video_error(), control.video_error());
                        let samples = |machine: &mut Emulator| {
                            machine
                                .drain_audio_samples()
                                .into_iter()
                                .map(f32::to_bits)
                                .collect::<Vec<_>>()
                        };
                        let actual = samples(&mut restored);
                        let expected = samples(&mut control);
                        // Raw states restart host audio; frontend states retain it.
                        if frontend {
                            assert_eq!(actual, expected);
                        }
                        hash.update(restored.sound_cpu.cycles().to_le_bytes());
                    }
                }
                if phases == 3 {
                    break;
                }
            }
            machine.step_instruction().unwrap();
        }
        assert_eq!(phases, 3, "both pending sound phases not reached");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_actual_interrupt_reset_and_rmw_field_contract() {
    illegal_unary_stack_entry();
    let mut hash = Sha256::new();
    for concurrent in [false, true] {
        let mut actual = fixture(concurrent, false, false, 0, false);
        actual.cpu.registers.a[0] = 0xff0020;
        actual.cpu.registers.set_status_register(0x2000);
        actual.bus.irq_level = 3;
        let mut control = actual.clone();
        let expected = control.step_field();
        #[cfg(feature = "first-failure-diagnostic")]
        let capture = crate::diagnostic::Capture::begin().unwrap();
        let result = actual.step_field();
        #[cfg(feature = "first-failure-diagnostic")]
        let report = capture.finish(&actual).unwrap();
        assert_eq!(result, expected);
        assert!(matches!(
            result,
            Err(MachineFault::Main(TimingFault::Bus {
                direction: Direction::Read,
                source: BusError::UnsupportedAccess {
                    address: 0x400000,
                    ..
                },
                ..
            }))
        ));
        assert_eq!(actual.work_ram()[0x20], 0);
        assert_eq!(actual.reset_pulses(), 1);
        #[cfg(feature = "first-failure-diagnostic")]
        {
            use crate::diagnostic::Callback;
            assert!(report.complete);
            for callback in [
                Callback::Read,
                Callback::Write,
                Callback::Idle,
                Callback::Acknowledge,
                Callback::BeginRmw,
                Callback::EndRmw,
                Callback::BeginReset,
                Callback::EndReset,
            ] {
                assert!(
                    report
                        .history
                        .iter()
                        .flatten()
                        .any(|record| record.callback == callback),
                    "{callback:?}"
                );
            }
        }
        compare(&mut actual, &mut control, &mut hash);
    }
    let digest = hash.finalize();
    let receipt: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    #[cfg(not(target_arch = "wasm32"))]
    println!("diagnostic-callback-contract-v1 {receipt}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("diagnostic-callback-contract-v1 {}", receipt);
}

fn illegal_unary_stack_entry() {
    for concurrent in [false, true] {
        for sp in [0x00fffe00_u32, 0x444e4c44, 0x00800000, 0x00800002] {
            let unclaimed = sp == 0x444e4c44;
            let unmapped = sp == 0x00800002;
            let pc: u32 = if unclaimed { 0 } else { 0x100 };
            let mut image = vec![0; 0x400];
            image[..4].copy_from_slice(&sp.to_be_bytes());
            image[4..8].copy_from_slice(&pc.to_be_bytes());
            image[0x10..0x14].copy_from_slice(&0x200_u32.to_be_bytes());
            if !unclaimed {
                image[0x100..0x104].copy_from_slice(&[0x44, 0x4e, 0x4c, 0x44]);
            }
            image[0x200..0x204].copy_from_slice(&[0x4e, 0x71, 0x60, 0xfe]);
            let mut machine = Emulator::new(
                image,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: concurrent,
                },
            )
            .unwrap();
            let registers = machine.registers();
            let before_rom = machine.rom_bytes().to_vec();
            #[cfg(feature = "first-failure-diagnostic")]
            let capture = crate::diagnostic::Capture::begin().unwrap();
            let result = machine.step_instruction();
            #[cfg(feature = "first-failure-diagnostic")]
            let report = capture.finish(&machine).unwrap();
            if unmapped {
                assert!(matches!(result, Err(MachineFault::Main(TimingFault::Bus {
                    access, direction: Direction::Write, source: BusError::UnsupportedAccess {address: 0x800000, direction: Direction::Write}
                })) if access.logical_address == 0x800000 && access.bus_address == 0x800000 && access.kind == zeff_m68k::bus::AccessKind::Stack && access.function_code == zeff_m68k::bus::FunctionCode::SupervisorData && access.lanes == ByteLanes::Both));
                assert_eq!(machine.registers(), registers);
                assert_eq!(machine.cpu_clock().0, 16);
                assert_eq!(machine.timing_snapshot().now().get(), 112);
                assert_eq!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Frozen))
                );
            } else {
                result.unwrap();
                assert_eq!(machine.registers().pc, 0x200);
                assert_eq!(machine.registers().ssp, sp - 6);
                assert_eq!(machine.cpu_clock().0, 42);
                assert_eq!(machine.timing_snapshot().now().get(), 294);
                assert_eq!(machine.queued_words(), Some([0x4e71, 0x60fe]));
                if sp == 0x00fffe00 {
                    assert_eq!(&machine.work_ram()[0xfdfa..0xfe00], &[0x27, 0, 0, 0, 1, 0]);
                } else {
                    assert!(machine.work_ram().iter().all(|byte| *byte == 0));
                    assert_eq!(machine.peek((sp - 2) & 0xffffff), None);
                }
            }
            assert_eq!(machine.rom_bytes(), before_rom);
            #[cfg(feature = "first-failure-diagnostic")]
            {
                use crate::diagnostic::{Callback, Outcome};
                assert!(report.complete);
                assert_eq!(report.total_callbacks, if unmapped { 2 } else { 9 });
                let transfers: Vec<_> = report
                    .history
                    .iter()
                    .flatten()
                    .filter(|r| r.access.is_some())
                    .collect();
                assert_eq!(transfers.len(), if unmapped { 1 } else { 7 });
                let first = transfers[0];
                assert_eq!(first.callback, Callback::Write);
                assert_eq!(first.access.unwrap().logical_address, sp - 2);
                assert_eq!(first.write_value, Some(pc as u16));
                assert_eq!(
                    first.outcome,
                    Some(if unmapped {
                        Outcome::UnsupportedAccess {
                            address: 0x800000,
                            direction: Direction::Write,
                        }
                    } else {
                        Outcome::Complete
                    })
                );
            }
        }
    }
}

#[cfg(feature = "first-failure-diagnostic")]
fn failed_sound_preparation_is_pure() {
    use crate::diagnostic::Capture;
    let cases = [
        (
            &[0x3a, 0x00, 0x7f, 0x76][..],
            0x7f00,
            Direction::Read,
            None,
            0,
            0x3a,
        ),
        (
            &[0x3e, 0x5a, 0x32, 0x9c, 0xc7, 0x76][..],
            0xc79c,
            Direction::Write,
            Some(0x5a),
            2,
            0x32,
        ),
        (
            &[0xdd, 0x21, 0x00, 0x7f, 0xdd, 0x7e, 0x00, 0x76][..],
            0x7f00,
            Direction::Read,
            None,
            4,
            0xdd,
        ),
    ];
    for (program, address, direction, value, pc, opcode) in cases {
        let mut actual = fixture(true, false, false, 1, false);
        actual.bus.sound.ram[..program.len()].copy_from_slice(program);
        actual.bus.sound.bank = 0x180;
        let mut control = actual.clone();
        let expected_fault = SoundFault::UnsupportedAccess { address, direction };
        let mut outcomes = Vec::new();
        for _ in 0..64 {
            outcomes.push(control.step_instruction());
            if outcomes.last().unwrap().is_err() {
                break;
            }
        }
        assert_eq!(
            outcomes.last(),
            Some(&Err(MachineFault::Sound(expected_fault)))
        );
        let capture = write_watch::sound_failure_capture();
        for expected in outcomes {
            assert_eq!(actual.step_instruction(), expected);
        }
        assert_eq!(actual, control);
        assert!(actual.diagnostic_pending_sound().is_none());
        let report = capture.finish(&actual).unwrap();
        assert!(report.complete);
        let writes = report.write_history.as_ref().unwrap();
        assert!(writes.complete && writes.observations.is_empty());
        assert_eq!(report.schema, 2);
        assert!(report.main_clocked_bus_only && report.service_scheduler_unavailable);
        let failure = report.first_sound_failure.unwrap();
        assert_eq!(failure.kind, "SPECULATIVE_PREPARATION");
        assert_eq!(failure.fault, expected_fault);
        assert_eq!(failure.access.local_address, address);
        assert_eq!(failure.access.direction, direction);
        assert_eq!(failure.access.write_value, value);
        assert_eq!(failure.access.bank, 0x180);
        assert_eq!(
            failure.access.translated_main,
            (address >= 0x8000).then_some(0xc0479c)
        );
        assert!(failure.access.ordinal > 0 && failure.access.ordinal <= failure.candidate_accesses);
        assert!(failure.preparation.bounded);
        assert!(failure.before_state.transfer_pending());
        assert_eq!(
            (failure.returned_segment.pc, failure.returned_segment.opcode),
            (pc, opcode)
        );
        assert_eq!(failure.before_registers, actual.sound_cpu.regs());
        assert_eq!(failure.before_cycles, actual.sound_cpu.cycles());
        assert_eq!(
            failure.after_cycles - failure.before_cycles,
            u64::from(failure.returned_segment.cycles)
        );
        let terminal = failure.terminal.unwrap();
        assert_eq!(terminal.kind, "CAPTURE_FINISH_OBSERVATION");
        assert_eq!(
            terminal.master_ticks,
            report.terminal.context.bus.master_ticks
        );
        assert_eq!(terminal.vdp_ticks, report.terminal.context.bus.vdp_ticks);
        assert_eq!(terminal.command, report.terminal.context.bus.vdp_command);
        assert_eq!(terminal.cache_ready, actual.vdp().read_cache().is_ready());
        assert_eq!(
            terminal.buffered_word,
            actual.vdp().read_cache().buffered_word()
        );
        let mut hash = Sha256::new();
        compare(&mut actual, &mut control, &mut hash);
        drop(Capture::begin().unwrap());
        let fresh = Capture::begin().unwrap().finish(&actual).unwrap();
        assert!(fresh.first_sound_failure.is_none());
    }
}
