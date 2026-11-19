use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_literal_rows_use_actual_inputs_and_same_value_write() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let (mut core, count) = machine(
            region,
            &[0x3e, 0x5a, 0x32, 0, 0x10, 0, 0x76],
            &[],
            true,
            false,
        );
        let baseline = core.sound_cpu().cycles();
        let mut ordinary = core.clone();
        let mut trace = SoundTraceContinuation::default();
        let mut output = store();
        for _ in 0..count + 32 {
            assert_eq!(
                step(&mut core, &mut trace, &mut output),
                ordinary.step_instruction()
            );
        }
        exact(&core, &ordinary);
        let actual = rows(&output);
        assert_eq!(
            actual[..4]
                .iter()
                .map(|row| row.instruction_bytes().to_vec())
                .collect::<Vec<_>>(),
            [vec![0x3e, 0x5a], vec![0x32, 0, 0x10], vec![0], vec![0x76]]
        );
        assert_eq!(
            actual[..4]
                .iter()
                .map(|row| row.boundary.unwrap().cpu_clocks)
                .collect::<Vec<_>>(),
            [7, 13, 4, 4]
        );
        assert_eq!(
            actual[..4].iter().map(|row| row.cycle).collect::<Vec<_>>(),
            [baseline, baseline + 7, baseline + 20, baseline + 24]
        );
        assert!(
            actual
                .iter()
                .all(|row| row.observation.frame_knowledge == TraceFrameKnowledge::Unknown)
        );
        assert!(
            actual[..4]
                .iter()
                .all(|row| row.observation.end == TraceObservationEnd::Retired)
        );
        assert_eq!(actual[1].writes().len(), 1);
        assert_eq!(
            (
                actual[1].writes()[0].address,
                actual[1].writes()[0].old_value,
                actual[1].writes()[0].new_value
            ),
            (0x1000, None, 0x5a)
        );
        assert!(
            actual[0]
                .register_deltas()
                .iter()
                .any(|delta| delta.register == 0 && delta.value == 0x5a)
        );
        assert!(core.passive_sound_trace_rejection().is_some());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_zero_clock_inputs_and_long_prefix_history_do_not_merge_by_cycles() {
    let (mut core, count) = machine(
        Region::NtscUs,
        &[0x09, 0x06, 2, 0x10, 0, 0x18, 0, 0x76],
        &[],
        true,
        false,
    );
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count + 32);
    let actual = rows(&output);
    assert_eq!(
        actual[..5]
            .iter()
            .map(|row| row.instruction_bytes().to_vec())
            .collect::<Vec<_>>(),
        [
            vec![0x09],
            vec![0x06, 2],
            vec![0x10, 0],
            vec![0x18, 0],
            vec![0x76]
        ]
    );
    assert_eq!(
        actual[..5]
            .iter()
            .map(|row| row.boundary.unwrap().cpu_clocks)
            .collect::<Vec<_>>(),
        [11, 7, 13, 12, 4]
    );
    assert!(events(&mut trace, &mut output).iter().any(|event| matches!(event.fact, Fact::Retired(e) if e.consumed.is_some() && e.cpu_before == e.cpu_after)));
    let mut program = vec![0xdd; 17];
    program.extend([0, 0x76]);
    let (mut core, count) = machine(Region::NtscUs, &program, &[], true, false);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count + 64);
    let actual = rows(&output);
    assert_eq!(actual[0].instruction_bytes(), &[0xdd; 16]);
    assert_eq!(actual[0].instruction_overflow, 2);
    assert_eq!(actual[0].boundary.unwrap().cpu_clocks, 72);
    assert!(actual.iter().any(|row| row.instruction_bytes().is_empty()
        && row.boundary.unwrap().kind == TraceBoundaryKind::Stopped));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_real_main_operand_patch_cannot_rewrite_a_retired_sample() {
    let (mut core, count) = machine(Region::NtscUs, &[0x3e, 0x5a, 0x76], &[], false, false);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count);
    assert!(core.sound_bus_granted());
    core.debug_write_ports_captured(
        &[port(0xa00001, 0xa6), port(0xa11100, 0)],
        &mut trace.attach(&mut output),
    )
    .unwrap();
    for _ in 0..32 {
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    let row = *output
        .iter()
        .find(|row| row.pc == 0 && row.instruction_bytes() == [0x3e, 0xa6])
        .unwrap();
    assert_eq!(row.observation.end, TraceObservationEnd::Retired);
    assert_eq!(core.sound_cpu().regs().a, 0xa6);
    core.debug_write_ports_captured(&[port(0xa11100, 1)], &mut trace.attach(&mut output))
        .unwrap();
    assert!(core.sound_bus_requested());
    for _ in 0..32 {
        if core.sound_bus_granted() {
            break;
        }
        step(&mut core, &mut trace, &mut output).unwrap();
    }
    assert!(core.sound_bus_granted() && !core.sound_reset_asserted());
    assert!(core.bus.timed_port_sound_phase().unwrap().0.bus_ack);
    core.debug_write_ports_captured(&[port(0xa00001, 0x91)], &mut trace.attach(&mut output))
        .unwrap();
    assert_eq!(core.peek_sound(1), Some(0xa6));
    // The port write publishes to SOUND on the next committed physical half.
    step(&mut core, &mut trace, &mut output).unwrap();
    assert!(core.sound_bus_requested() && core.sound_bus_granted());
    assert_eq!(core.peek_sound(1), Some(0x91));
    assert_eq!(core.sound_cpu().regs().a, 0xa6);
    assert_eq!(
        output
            .iter()
            .find(|current| current.sequence == row.sequence),
        Some(&row)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_producer_psg_receipts_do_not_certify_vdp_strobes() {
    for vdp in [false, true] {
        let mut sound = vec![0; 8];
        sound.extend([
            0x21,
            if vdp { 0 } else { 0x11 },
            0x7f,
            0x3e,
            0x22,
            0x77,
            0x77,
            0x76,
        ]);
        let (mut core, count) = machine(Region::NtscUs, &sound, &[], false, false);
        let mut trace = SoundTraceContinuation::default();
        let mut output = store();
        upload(&mut core, &mut trace, &mut output, count);
        core.debug_write_ports_captured(
            &[
                control(0x8f02),
                control(0x4000),
                control(0),
                port(0xa11100, 0),
            ],
            &mut trace.attach(&mut output),
        )
        .unwrap();
        let mut receipts = Vec::new();
        let mut vdp_receipts = Vec::new();
        for _ in 0..64 {
            step(&mut core, &mut trace, &mut output).unwrap();
            for event in events(&mut trace, &mut output) {
                if matches!(event.fact, Fact::PsgAccepted(_)) && !receipts.contains(&event) {
                    receipts.push(event);
                }
                if matches!(event.fact, Fact::VdpAccepted(_)) && !vdp_receipts.contains(&event) {
                    vdp_receipts.push(event);
                }
            }
        }
        let actual = rows(&output);
        let stores: Vec<_> = actual
            .iter()
            .filter(|row| row.instruction_bytes() == [0x77])
            .collect();
        assert_eq!(stores.len(), 2);
        if vdp {
            assert!(
                stores
                    .iter()
                    .all(|row| row.observation.end == TraceObservationEnd::Retired
                        && row.writes().len() == 1
                        && row.writes()[0].address == 0x7f00
                        && row.writes()[0].new_value == 0x22)
            );
            assert!(receipts.is_empty());
            assert!(vdp_receipts.len() >= 2);
            assert!(vdp_receipts.iter().all(|event| matches!(event.fact, Fact::VdpAccepted(receipt) if receipt.kind == crate::sound_trace::physical::vdp::Kind::Data)));
        } else {
            assert!(
                stores
                    .iter()
                    .all(|row| row.observation.end == TraceObservationEnd::Retired
                        && row.writes().len() == 1
                        && row.writes()[0].address == 0x7f11
                        && row.writes()[0].new_value == 0x22)
            );
            assert_eq!(receipts.len(), 2);
            assert!(vdp_receipts.is_empty());
        }
        assert!(core.passive_sound_trace_rejection().is_some());
    }
}
