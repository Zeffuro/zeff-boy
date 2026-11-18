use super::*;

#[cfg_attr(test, test)]
pub(crate) fn prepared_local_epoch_materializes_before_the_reset_sample() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = machine(&[0x4e71; 8], true, false, region);
        actual.bus.sound.ram[..2].copy_from_slice(&[0x18, 0xfe]);
        actual.sound_schedule.begin_epoch_field();
        for _ in 0..7 {
            seed(&mut actual);
        }
        assert!(actual.sound_schedule.epoch.is_some());
        assert!(actual.sound_schedule.pending.is_none());
        let mut control = actual.clone();
        control.sound_schedule.finish_epoch_field(
            &mut control.sound_cpu,
            &control.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
        let pending = control.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.deadline > control.bus.now.get());
        assert!(!pending.any_bank_access && pending.bank_access.is_none());
        assert!(control.sound_cpu.cycles() > actual.sound_cpu.cycles());
        let request = access(SOUND_RESET, AccessKind::Data, ByteLanes::Both);
        let result = read(&mut actual, request);
        assert_eq!(result.result, Ok(0x1234));
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
        assert_eq!(read(&mut control, request), result);
        actual.sound_schedule.finish_epoch_field(
            &mut actual.sound_cpu,
            &actual.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
        assert_eq!(actual, control);
        assert_eq!(read(&mut actual, request).result, failure(SOUND_RESET));
    }
}

#[cfg_attr(test, test)]
pub(crate) fn active_local_clr_and_move_use_the_actual_prefetch() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for (opcode, following) in [(0x3039, 0x7245), (0x4279, 0x4e71)] {
            let mut actual = machine(
                &[opcode, 0x00a1, 0x1200, following, 0x60fe, 0x4e71],
                true,
                false,
                region,
            );
            actual.bus.sound.ram[..2].copy_from_slice(&[0x18, 0xfe]);
            assert!(!actual.bus.sound.requested && !actual.bus.sound.granted);
            let before = actual.bus.now.get();
            let mut public = actual.clone();
            public.step_instruction().unwrap();
            let mut scheduled = ScheduledBus::new(
                &mut actual.bus,
                &mut actual.sound_cpu,
                &mut actual.sound_schedule,
            );
            let mut trace = Trace {
                bus: &mut scheduled,
                reads: Vec::new(),
                writes: Vec::new(),
            };
            actual.cpu.step(&mut trace).unwrap();
            let reads: Vec<_> = trace
                .reads
                .iter()
                .filter(|v| v.0 == AccessKind::Data)
                .collect();
            assert_eq!(reads.len(), 1);
            assert_eq!(
                (reads[0].1, reads[0].2, reads[0].3),
                (SOUND_RESET, ByteLanes::Both, following)
            );
            assert_eq!(trace.writes.len(), usize::from(opcode == 0x4279));
            if opcode == 0x3039 {
                assert_eq!(actual.registers().d[0], u32::from(following));
                assert!(!actual.bus.sound.reset_asserted);
                assert_eq!(actual.bus.now.get() - before, 112);
            } else {
                assert!(actual.bus.sound.reset_asserted);
                assert_eq!(actual.bus.now.get() - before, 140);
            }
            assert_eq!(actual, public);
        }
    }
}

#[cfg_attr(test, test)]
pub(crate) fn local_deadlines_and_grants_consume_history_once() {
    for phase in 0..15 {
        for program in [[0x3a, 0, 1], [0x32, 0, 1]] {
            for offset in [-1i64, 0, 1, 27] {
                for requested in [false, true] {
                    let mut actual = pending(&program, 300 + phase);
                    let deadline = actual.sound_schedule.pending.as_ref().unwrap().deadline;
                    let end = deadline.checked_add_signed(offset).unwrap();
                    position(&mut actual, end - 28);
                    actual.bus.sound.request(requested);
                    let mut control = actual.clone();
                    let result = read(
                        &mut actual,
                        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
                    );
                    assert_eq!(result.result, Ok(0x1234));
                    assert_eq!(result.wait_clocks, 0);
                    assert_eq!(actual.bus.now.get(), end);
                    assert_eq!(actual.bus.prefetch, Prefetch::Empty);
                    read(
                        &mut control,
                        access(0xff0200, AccessKind::Data, ByteLanes::Both),
                    )
                    .result
                    .unwrap();
                    assert_eq!(actual, control);
                    assert_eq!(
                        read(
                            &mut actual,
                            access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
                        )
                        .result,
                        failure(SOUND_RESET)
                    );
                }
            }
        }
    }
}

#[cfg_attr(test, test)]
pub(crate) fn foreign_driver_history_and_faults_still_reject_reset() {
    for offset in [-1i64, 0, 1] {
        let mut actual = pending(&[0x32, 0, 0x80, 0], 300);
        let pending = actual.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access && pending.bank_access.is_none());
        let end = pending.deadline.checked_add_signed(offset).unwrap();
        position(&mut actual, end - 28);
        actual.bus.ram[0x8000] = 0xde;
        let result = read(
            &mut actual,
            access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
        );
        assert_eq!(result.result, failure(SOUND_RESET));
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
        if offset > 0 {
            let last = actual.sound_schedule.pending.as_ref().unwrap();
            assert!(!last.any_bank_access && last.bank_access.is_none());
            assert_eq!(actual.bus.ram[0x8000], 0);
        }
    }
    for fault in [false, true] {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        position(&mut actual, 350);
        if fault {
            actual.sound_schedule.fault = Some(SoundFault::ClockOverflow);
        } else {
            actual
                .sound_schedule
                .pending
                .as_mut()
                .unwrap()
                .any_bank_access = true;
        }
        assert_eq!(
            read(
                &mut actual,
                access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
            )
            .result,
            failure(SOUND_RESET)
        );
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
    }
}

#[cfg_attr(test, test)]
pub(crate) fn selectors_and_incomplete_history_remain_rejected_while_active() {
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        for offset in (0..=0xfe).step_by(2) {
            if offset == 0 && lanes == ByteLanes::Both {
                continue;
            }
            let mut actual = pending(&[0x3a, 0, 1], 300);
            position(&mut actual, 350);
            assert_eq!(
                read(
                    &mut actual,
                    access(SOUND_RESET + offset, AccessKind::Data, lanes)
                )
                .result,
                if offset == 0 && lanes == ByteLanes::Upper {
                    Ok(0x1200)
                } else {
                    failure(SOUND_RESET + offset)
                }
            );
            assert_eq!(actual.bus.prefetch, Prefetch::Empty);
        }
    }
    for kind in [
        AccessKind::Opcode,
        AccessKind::Extension,
        AccessKind::ResetVector,
        AccessKind::Vector,
        AccessKind::Stack,
        AccessKind::Discard,
        AccessKind::Prefetch,
        AccessKind::InterruptAcknowledge,
    ] {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        position(&mut actual, 350);
        assert_eq!(
            read(&mut actual, access(SOUND_RESET, kind, ByteLanes::Both)).result,
            failure(SOUND_RESET)
        );
    }
    for history in [
        Prefetch::Empty,
        Prefetch::UpperOnly(0),
        Prefetch::UpperOnly(0xff),
    ] {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        position(&mut actual, 350);
        actual.bus.prefetch = history;
        assert_eq!(
            read(
                &mut actual,
                access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
            )
            .result,
            failure(SOUND_RESET)
        );
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
    }
}

#[cfg_attr(test, test)]
pub(crate) fn failed_clocks_and_dma_waits_do_not_recover_reset_history() {
    let mut reserved = pending(&[0x32, 0, 0x80], 300);
    reserved
        .sound_schedule
        .pending
        .as_mut()
        .unwrap()
        .bank_access = Some(crate::hardware::sound::BankAccess {
        address: 0xff8000,
        direction: Direction::Write,
        value: 0,
    });
    position(&mut reserved, 470);
    let result = read(
        &mut reserved,
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
    );
    assert!(result.wait_clocks > 0);
    assert_eq!(result.result, failure(SOUND_RESET));
    assert_eq!(reserved.bus.prefetch, Prefetch::Empty);

    let mut dma = machine(&[0x4e71; 4], true, true, Region::NtscUs);
    for (index, value) in [
        (15, 2),
        (19, 17),
        (20, 0),
        (21, 0),
        (22, 0),
        (23, 0),
        (1, 0x14),
    ] {
        dma.bus.advance_machine(1).unwrap();
        let write = dma
            .bus
            .vdp
            .queue_register(index, value, Region::NtscUs)
            .unwrap();
        dma.bus
            .advance_machine(write.publish_at.get() - dma.bus.now.get())
            .unwrap();
    }
    seed(&mut dma);
    dma.bus.vdp.write_control(0x4000, Region::NtscUs).unwrap();
    dma.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
    let result = read(
        &mut dma,
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
    );
    assert!(result.wait_clocks > 0);
    assert_eq!(result.result, failure(SOUND_RESET));

    let mut actual = machine(&[0x4e71; 4], true, false, Region::NtscUs);
    seed(&mut actual);
    let saved = actual.clone();
    let clock = CpuClocks(actual.bus.cpu_clock.0 + 1);
    let result = ScheduledBus::new(
        &mut actual.bus,
        &mut actual.sound_cpu,
        &mut actual.sound_schedule,
    )
    .read_cycle(
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
        clock,
    );
    assert!(matches!(result.result, Err(BusError::ClockMismatch { .. })));
    assert_eq!(actual, saved);
    actual.bus.now = MasterTicks::new(u64::MAX - 1);
    let saved = actual.clone();
    assert_eq!(
        read(
            &mut actual,
            access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
        )
        .result,
        Err(BusError::ClockOverflow)
    );
    assert_eq!(actual, saved);
}

#[cfg_attr(test, test)]
pub(crate) fn active_reset_read_raw_frontend_and_clone_continuations_match() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = machine(
            &[0x3039, 0x00a1, 0x1200, 0x7245, 0x60fe, 0x4e71],
            true,
            false,
            region,
        );
        source.bus.sound.ram[..2].copy_from_slice(&[0x18, 0xfe]);
        source.set_sample_rate(48_000);
        for frontend in [false, true] {
            let state = if frontend {
                source.encode_frontend_state().unwrap()
            } else {
                source.encode_state().unwrap()
            };
            let mut expected = source.clone();
            let mut restored = source.clone();
            restored.step_instruction().unwrap();
            if frontend {
                restored.decode_frontend_state(&state).unwrap();
            } else {
                restored.decode_state(&state).unwrap();
                expected.decode_state(&state).unwrap();
            }
            for _ in 0..3 {
                expected.step_instruction().unwrap();
                restored.step_instruction().unwrap();
            }
            assert_eq!(restored.registers().d[0], 0x7245);
            for _ in 0..2 {
                expected.step_field().unwrap();
                restored.step_field().unwrap();
                assert_eq!(
                    restored.encode_state().unwrap(),
                    expected.encode_state().unwrap()
                );
                assert_eq!(
                    restored.drain_audio_samples(),
                    expected.drain_audio_samples()
                );
                assert_eq!(restored.framebuffer(), expected.framebuffer());
            }
        }
    }
}
