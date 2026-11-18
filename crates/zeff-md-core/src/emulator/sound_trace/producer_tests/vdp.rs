use super::*;
use crate::sound_trace::physical::vdp::{Kind, raw::Record};
use zeff_emu_common::debug::TraceWriteKind;

mod oracle;
fn raw(trace: &mut SoundTraceContinuation, output: &mut InstructionTraceStore) -> Vec<Record> {
    trace
        .attach(output)
        .physical_journal()
        .vdp_witness()
        .to_vec()
}
fn held(region: Region, sound: &[u8]) -> (Emulator, SoundTraceContinuation, InstructionTraceStore) {
    let (mut core, count) = machine(region, sound, &[], false, false);
    let mut trace = SoundTraceContinuation::default();
    let mut output = store();
    upload(&mut core, &mut trace, &mut output, count);
    assert!(core.sound_bus_requested() && core.sound_bus_granted() && !core.sound_reset_asserted());
    for (address, &value) in sound.iter().enumerate() {
        assert_eq!(
            core.peek_sound(address as u16),
            Some(value),
            "uploaded SOUND byte {address}"
        );
    }
    core.debug_write_ports_captured(
        &[
            control(0x8104),
            control(0x8f02),
            control(0x4000),
            control(0),
        ],
        &mut trace.attach(&mut output),
    )
    .unwrap();
    trace
        .attach(&mut output)
        .physical_journal()
        .start_vdp_witness();
    (core, trace, output)
}
fn run(
    core: &mut Emulator,
    trace: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
    halt: u32,
) -> Vec<Event> {
    let mut ordinary = core.clone();
    let mut all = events(trace, output);
    core.debug_write_ports_captured(&[port(0xa11100, 0)], &mut trace.attach(output))
        .unwrap();
    ordinary.debug_write_ports(&[port(0xa11100, 0)]).unwrap();
    let mut reached = false;
    for _ in 0..128 {
        assert_eq!(step(core, trace, output), ordinary.step_instruction());
        for event in events(trace, output) {
            if !all.contains(&event) {
                all.push(event);
            }
        }
        if output
            .iter()
            .any(|row| row.pc == halt && row.instruction_bytes() == [0x76])
        {
            reached = true;
            break;
        }
    }
    assert!(reached);
    for _ in 0..4 {
        assert_eq!(step(core, trace, output), ordinary.step_instruction());
        for event in events(trace, output) {
            if !all.contains(&event) {
                all.push(event);
            }
        }
    }
    exact(core, &ordinary);
    all
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_vdp_actual_ack_intake_joins_complete_raw_chronology_both_lanes() {
    let sound = [
        0x3e, 0, 0x32, 4, 0x7f, 0x3e, 0x5a, 0x32, 0, 0x7f, 0x3e, 0x5a, 0x32, 1, 0x7f, 0x76,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let (mut core, mut trace, mut output) = held(region, &sound);
        let events = run(&mut core, &mut trace, &mut output, 15);
        let proof = oracle::exact(&raw(&mut trace, &mut output), &events);
        assert_eq!(
            proof
                .iter()
                .map(|entry| (entry.0, entry.1))
                .collect::<Vec<_>>(),
            [
                (0x7f04, Kind::ControlFirst),
                (0x7f00, Kind::Data),
                (0x7f01, Kind::Data)
            ]
        );
        for (pc, address, value) in [(2, 0x7f04, 0), (7, 0x7f00, 0x5a), (12, 0x7f01, 0x5a)] {
            let row = output.iter().find(|row| row.pc == pc).unwrap();
            assert_eq!(row.observation.end, TraceObservationEnd::Retired);
            assert_eq!(row.writes().len(), 1);
            assert_eq!(
                (
                    row.writes()[0].address,
                    row.writes()[0].new_value,
                    row.writes()[0].kind
                ),
                (address, value, TraceWriteKind::Memory)
            );
        }
        assert!(core.passive_sound_trace_rejection().is_some());
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_vdp_actual_upper_psg_ack_has_bus_disposition_without_intake() {
    let sound = [0x3e, 0x22, 0x32, 0x10, 0x7f, 0x76];
    let (mut core, mut trace, mut output) = held(Region::NtscUs, &sound);
    let events = run(&mut core, &mut trace, &mut output, 5);
    let witness = raw(&mut trace, &mut output);
    assert!(witness.iter().any(|record| matches!(
        record.event,
        crate::sound_trace::physical::vdp::raw::Event::Primitive(input)
            if input.psg == [true, false] && input.lanes == [false, false]
    )));
    let proof = oracle::exact(&witness, &events);
    assert_eq!(proof.len(), 1);
    assert_eq!((proof[0].0, proof[0].1), (0x7f10, Kind::DisconnectedPsg));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.fact, Fact::PsgAccepted(_)))
    );
    let row = output.iter().find(|row| row.pc == 2).unwrap();
    assert_eq!(row.observation.end, TraceObservationEnd::Retired);
    assert_eq!(row.writes().len(), 1);
    assert_eq!(
        (
            row.writes()[0].address,
            row.writes()[0].new_value,
            row.writes()[0].kind
        ),
        (0x7f10, 0x22, TraceWriteKind::Bus)
    );
    assert!(core.passive_sound_trace_rejection().is_some());
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_vdp_accepted_child_receipts_and_shadow_roll_back_on_late_port_fault() {
    let mut sound = vec![0; 8];
    sound.extend([0x3e, 0x5a, 0x32, 0, 0x7f, 0x76]);
    let (mut core, mut trace, mut output) = held(Region::NtscUs, &sound);
    let original = core.clone();
    let before = rows(&output);
    let raw_before = raw(&mut trace, &mut output);
    let journal_before = events(&mut trace, &mut output);
    let mut commands = vec![port(0xa11100, 0)];
    commands.extend([control(0x8f02); 40]);
    let fork = trace.fork(&mut core, &mut output);
    core.debug_write_ports_captured(&commands, &mut trace.attach(&mut output))
        .unwrap();
    let child_journal = events(&mut trace, &mut output);
    assert!(child_journal.iter().any(|event| matches!(
        event.fact,
        Fact::VdpAccepted(receipt)
            if receipt.kind == Kind::Data && receipt.binding.address == 0x7f00
                && receipt.bus_address == 0xc00000 && receipt.word == 0x5a5a
    )));
    let store = output.iter().find(|row| row.pc == 10).unwrap();
    assert_eq!(store.instruction_bytes(), &[0x32, 0, 0x7f]);
    assert_eq!(store.observation.end, TraceObservationEnd::Retired);
    assert_eq!(store.writes().len(), 1);
    assert_eq!(
        (
            store.writes()[0].address,
            store.writes()[0].new_value,
            store.writes()[0].kind
        ),
        (0x7f00, 0x5a, TraceWriteKind::Memory)
    );
    let child = rows(&output);
    let child_raw = raw(&mut trace, &mut output);
    let child_machine = core.clone();
    fork.restore(&mut core, &mut trace, &mut output);
    reject_late_interlace(&mut core, &commands, &mut trace, &mut output);
    exact(&core, &original);
    assert_eq!(rows(&output), before);
    assert_eq!(raw(&mut trace, &mut output), raw_before);
    assert_eq!(events(&mut trace, &mut output), journal_before);
    core.debug_write_ports_captured(&commands, &mut trace.attach(&mut output))
        .unwrap();
    exact(&core, &child_machine);
    assert_eq!(rows(&output), child);
    assert_eq!(raw(&mut trace, &mut output), child_raw);
    assert_eq!(events(&mut trace, &mut output), child_journal);
}
