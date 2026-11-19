use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_accepted_m1_sample_hold_replays_once_through_paired_fork() {
    let mut sound = vec![0; 8];
    sound.extend([0x3e, 0x5a, 0x76]);
    let (mut core, count) = machine(Region::NtscUs, &sound, &[0x4e72, 0x2700], true, false);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count + 1);
    let mut found = false;
    for _ in 0..256 {
        let raw = events(&mut trace, &mut output);
        if let Some(sample) = raw
            .iter()
            .rev()
            .find(|event| matches!(event.fact, Fact::Sampled(0x3e)))
            && let Some(id) = sample.occurrence
        {
            let consumed = raw.iter().any(|event| {
                event.occurrence == Some(id)
                    && matches!(event.fact, Fact::Retired(_) | Fact::ClassifiedRetired(_))
            });
            if !consumed {
                found = true;
                break;
            }
        }
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    assert!(found);
    assert!(!output.iter().any(|row| row.pc == 8));
    let machine = core.clone();
    let before = rows(&output);
    let raw = events(&mut trace, &mut output);
    let gap_fork = trace.fork(&mut core, &mut output);
    trace.disable(&mut output);
    output.set_enabled(true);
    for _ in 0..64 {
        step(&mut core, &mut trace, &mut output).unwrap();
        if output
            .iter()
            .any(|row| row.pc == 8 && row.instruction_bytes() == [0x3e, 0x5a])
        {
            break;
        }
    }
    let suffix = output
        .iter()
        .find(|row| row.pc == 8 && row.instruction_bytes() == [0x3e, 0x5a])
        .unwrap();
    assert_eq!(suffix.observation.start, TraceObservationStart::Suffix);
    gap_fork.restore(&mut core, &mut trace, &mut output);
    exact(&core, &machine);
    assert_eq!(rows(&output), before);
    assert_eq!(events(&mut trace, &mut output), raw);
    let fork = trace.fork(&mut core, &mut output);
    let mut steps = 0;
    while !output
        .iter()
        .any(|row| row.pc == 8 && row.instruction_bytes() == [0x3e, 0x5a])
    {
        assert!(steps < 64);
        step(&mut core, &mut trace, &mut output).unwrap();
        steps += 1;
    }
    let child = rows(&output);
    fork.restore(&mut core, &mut trace, &mut output);
    exact(&core, &machine);
    assert_eq!(rows(&output), before);
    assert_eq!(events(&mut trace, &mut output), raw);
    for _ in 0..steps {
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    assert_eq!(rows(&output), child);
    assert_eq!(
        output
            .iter()
            .filter(|row| row.pc == 8 && row.instruction_bytes() == [0x3e, 0x5a])
            .count(),
        1
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_first_committed_same_value_work_pulse_survives_abort_without_duplicates()
{
    let mut sound = vec![0; 8];
    sound.extend([0x21, 0, 0x80, 0x3e, 0x22, 0x77, 0x77, 0x76]);
    let (mut core, count) = machine(Region::NtscUs, &sound, &[0x4e72, 0x2700], true, true);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count + 1);
    let mut found = false;
    for _ in 0..256 {
        let raw = events(&mut trace, &mut output);
        if let Some(first) = raw.iter().find(
            |event| matches!(event.fact, Fact::WorkCommitted { tag, .. } if tag.logical == 0x8000),
        ) {
            let complete = raw.iter().any(|event| {
                event.occurrence == first.occurrence
                    && matches!(event.fact, Fact::Retired(e) if e.complete)
            });
            if !complete {
                found = true;
                break;
            }
        }
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    assert!(found);
    assert_eq!(core.work_ram()[0x8000], 0x22);
    assert!(!output.iter().any(|row| row.pc == 13));
    let machine = core.clone();
    let before = rows(&output);
    let fork = trace.fork(&mut core, &mut output);
    let mut steps = 0;
    while !output
        .iter()
        .any(|row| row.pc == 15 && row.instruction_bytes() == [0x76])
    {
        assert!(steps < 128);
        step(&mut core, &mut trace, &mut output).unwrap();
        steps += 1;
    }
    let child = rows(&output);
    for pc in [13, 14] {
        let row = child
            .iter()
            .find(|row| row.pc == pc && row.instruction_bytes() == [0x77])
            .unwrap();
        assert_eq!(row.writes().len(), 1);
        assert_eq!(
            (
                row.writes()[0].address,
                row.writes()[0].new_value,
                row.writes()[0].old_value
            ),
            (0x8000, 0x22, None)
        );
    }
    fork.restore(&mut core, &mut trace, &mut output);
    exact(&core, &machine);
    assert_eq!(rows(&output), before);
    for _ in 0..steps {
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    assert_eq!(rows(&output), child);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_actual_main_fault_retains_zero_clock_consumed_input() {
    let mut found = false;
    for delay in 0..=32 {
        let mut sound = vec![0; 8];
        sound.extend([0x09, 0x76]);
        let mut tail = vec![0x4e71; delay];
        tail.extend([0x3039, 0x00a1, 0x0020]);
        let (mut core, count) = machine(Region::NtscUs, &sound, &tail, true, false);
        let mut ordinary = core.clone();
        let mut trace = SoundTraceContinuation::default();
        let mut output = store();
        let mut failed = false;
        for _ in 0..count + delay + 2 {
            let actual = step(&mut core, &mut trace, &mut output);
            let normal = ordinary.step_instruction();
            assert_eq!(actual, normal);
            if actual.is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
        assert_eq!(core, ordinary);
        let boundary = output
            .iter()
            .find(|row| {
                row.pc == 8
                    && row.instruction_bytes() == [0x09]
                    && row.observation.end == TraceObservationEnd::Fault
            })
            .map(|row| row.boundary.unwrap());
        if let Some(boundary) = boundary
            && boundary.cpu_clocks == 0
        {
            assert!(boundary.machine_fault);
            assert!(events(&mut trace, &mut output).iter().any(|event| matches!(event.fact, Fact::Retired(e) if e.consumed.is_some_and(|byte| byte.address == 8 && byte.value == 0x09) && e.cpu_before == e.cpu_after)));
            found = true;
            break;
        }
    }
    assert!(found);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_timed_port_child_rows_roll_back_and_owned_resets_refuse_purely() {
    let (mut core, count) = machine(Region::NtscUs, &[0x3e, 0x5a, 0x76], &[], false, false);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count);
    let machine = core.clone();
    let before = rows(&output);
    let raw = events(&mut trace, &mut output);
    assert!(
        core.reset_cpu_sound_traced(&mut trace.attach(&mut output))
            .is_err()
    );
    assert!(
        core.warm_reset_sound_traced(&mut trace.attach(&mut output))
            .is_err()
    );
    assert!(!core.reset_sound_cpu_traced(&mut trace.attach(&mut output)));
    exact(&core, &machine);
    assert_eq!(rows(&output), before);
    assert_eq!(events(&mut trace, &mut output), raw);
    let mut writes = vec![port(0xa11100, 0)];
    writes.extend([control(0x8f02); 16]);
    let fork = trace.fork(&mut core, &mut output);
    core.debug_write_ports_captured(&writes, &mut trace.attach(&mut output))
        .unwrap();
    assert!(
        output
            .iter()
            .skip(before.len())
            .any(|row| row.instruction_bytes() == [0x3e, 0x5a]
                && row.observation.end == TraceObservationEnd::Retired)
    );
    fork.restore(&mut core, &mut trace, &mut output);
    reject_late_interlace(&mut core, &writes, &mut trace, &mut output);
    exact(&core, &machine);
    assert_eq!(rows(&output), before);
    assert_eq!(events(&mut trace, &mut output), raw);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_guest_entry_and_field_resume_share_committed_rows() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut sound = vec![0; 32];
        sound.extend([0x3e, 0x5a, 0x76]);
        let (mut core, count) = machine(region, &sound, &[], false, false);
        let mut trace = SoundTraceContinuation::default();
        let mut output = store();
        upload(&mut core, &mut trace, &mut output, count);
        let mut ordinary = core.clone();
        core.debug_write_ports_captured(&[port(0xa11100, 0)], &mut trace.attach(&mut output))
            .unwrap();
        ordinary.debug_write_ports(&[port(0xa11100, 0)]).unwrap();
        let before = output.len();
        let target = core.registers().pc;
        let _actual_call = core
            .begin_debug_call_captured(target, &mut trace.attach(&mut output))
            .unwrap();
        let _ordinary_call = ordinary.begin_debug_call(target).unwrap();
        assert!(output.len() > before);
        exact(&core, &ordinary);
        let actual_field = crate::emulator::field::FieldExecution::begin(&mut core).unwrap();
        let ordinary_field = crate::emulator::field::FieldExecution::begin(&mut ordinary).unwrap();
        let budget = std::num::NonZeroUsize::new(32).unwrap();
        assert!(
            actual_field
                .resume_captured(&mut core, budget, &mut trace.attach(&mut output))
                .unwrap()
                .is_some()
        );
        assert!(
            ordinary_field
                .resume(&mut ordinary, budget)
                .unwrap()
                .is_some()
        );
        exact(&core, &ordinary);
        assert!(output.iter().any(|row| row.pc == 32
            && row.instruction_bytes() == [0x3e, 0x5a]
            && row.observation.end == TraceObservationEnd::Retired));
        assert!(
            output
                .iter()
                .all(|row| row.observation.frame_knowledge == TraceFrameKnowledge::Unknown)
        );
        assert!(core.passive_sound_trace_rejection().is_some());
    }
}
