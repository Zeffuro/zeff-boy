use serde_json::json;
use zeff_emu_common::audio_trace::{AudioTraceEvent, AudioTraceSource, NesTraceWrite};
use zeff_emu_common::debug::{DebugEvent, InstructionTraceRecord, TraceExecMode};

use super::*;

fn source() -> Vec<u8> {
    let mut source = vec![0; 16 + 0x4000];
    source[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 0]);
    source
}

fn offset(pc: u16) -> usize {
    16 + (usize::from(pc) - 0x8000) % 0x4000
}

fn write(source: &mut [u8], pc: u16, bytes: &[u8]) {
    for (delta, byte) in bytes.iter().copied().enumerate() {
        source[offset(pc + delta as u16)] = byte;
    }
}

fn state(pc: u16, a: u8, x: u8, y: u8, sp: u8, cycle: u64) -> State {
    State {
        pc,
        a,
        x,
        y,
        sp,
        p: 0x24,
        cycle,
    }
}

fn record(before: State, bytes: &[u8]) -> InstructionTraceRecord {
    InstructionTraceRecord::new(
        TraceExecMode::Mos6502,
        u32::from(before.pc),
        Some((offset(before.pc) - 16) as u64),
        0,
        before.cycle,
        bytes,
    )
}

fn writer(pc: u16, address: u16, value: u8, cycle: u64) -> AudioTraceEvent<NesTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: u32::from(pc),
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: offset(pc) as u64,
            bit_reversed: false,
        },
        write: NesTraceWrite::Register {
            address,
            value,
            odd_cycle: false,
        },
    }
}

fn call(tracker: &mut Tracker<'_>, before: State, entry: State) {
    let record = record(before, &[0x20, entry.pc as u8, (entry.pc >> 8) as u8]);
    tracker.step(before, entry, Some(&record), &[]).unwrap();
}

#[test]
fn follows_entry_a_through_copy_branch_rom_read_and_y_store() {
    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    write(&mut rom, 0x8010, &[0xaa]);
    write(&mut rom, 0x8011, &[0xe0, 0x02]);
    write(&mut rom, 0x8013, &[0x90, 0x01]);
    write(&mut rom, 0x8016, &[0xea]);
    write(&mut rom, 0x8017, &[0xbd, 0x00, 0x81]);
    write(&mut rom, 0x801a, &[0xa8]);
    write(&mut rom, 0x801b, &[0xa9, 0x00]);
    write(&mut rom, 0x801d, &[0xea]);
    write(&mut rom, 0x801e, &[0x8c, 0x02, 0x40]);
    rom[offset(0x8101)] = 0x80;
    let mut tracker = Tracker::new(&rom).unwrap();

    let call_before = state(0x8000, 1, 7, 9, 0xfd, 7);
    let entry = state(0x8010, 1, 7, 9, 0xfb, 13);
    call(&mut tracker, call_before, entry);
    let tax_after = state(0x8011, 1, 1, 9, 0xfb, 15);
    tracker
        .step(entry, tax_after, Some(&record(entry, &[0xaa])), &[])
        .unwrap();
    let compare_after = state(0x8013, 1, 1, 9, 0xfb, 17);
    tracker
        .step(
            tax_after,
            compare_after,
            Some(&record(tax_after, &[0xe0, 0x02])),
            &[],
        )
        .unwrap();
    let branch_after = state(0x8016, 1, 1, 9, 0xfb, 19);
    tracker
        .step(
            compare_after,
            branch_after,
            Some(&record(compare_after, &[0x90, 0x01])),
            &[],
        )
        .unwrap();
    let nop_after = state(0x8017, 1, 1, 9, 0xfb, 21);
    tracker
        .step(
            branch_after,
            nop_after,
            Some(&record(branch_after, &[0xea])),
            &[],
        )
        .unwrap();
    let load_after = state(0x801a, 0x80, 1, 9, 0xfb, 25);
    tracker
        .step(
            nop_after,
            load_after,
            Some(&record(nop_after, &[0xbd, 0x00, 0x81])),
            &[],
        )
        .unwrap();
    let tay_after = state(0x801b, 0x80, 1, 0x80, 0xfb, 27);
    tracker
        .step(
            load_after,
            tay_after,
            Some(&record(load_after, &[0xa8])),
            &[],
        )
        .unwrap();
    let clobber_after = state(0x801d, 0, 1, 0x80, 0xfb, 29);
    tracker
        .step(
            tay_after,
            clobber_after,
            Some(&record(tay_after, &[0xa9, 0x00])),
            &[],
        )
        .unwrap();
    let final_before = state(0x801e, 0, 1, 0x80, 0xfb, 31);
    tracker
        .step(
            clobber_after,
            final_before,
            Some(&record(clobber_after, &[0xea])),
            &[],
        )
        .unwrap();
    let after_store = state(0x8021, 0, 1, 0x80, 0xfb, 35);
    tracker
        .step(
            final_before,
            after_store,
            Some(&record(final_before, &[0x8c, 0x02, 0x40])),
            &[(12, writer(0x801e, 0x4002, 0x80, 28))],
        )
        .unwrap();

    let links = tracker.finish();
    assert_eq!(links.as_array().unwrap().len(), 1);
    let link = &links[0];
    assert_eq!(link["call"]["pc"], 0x8000);
    assert_eq!(link["entry"]["pc"], 0x8010);
    assert_eq!(link["argument"]["register"], "a");
    assert_eq!(link["argument"]["value"], 1);
    assert_eq!(link["rom_read"]["index_register"], "x");
    assert_eq!(link["rom_read"]["index_value"], 1);
    assert_eq!(link["rom_read"]["address"], 0x8101);
    assert_eq!(link["event_index"], 12);
    assert_eq!(link["register"], 0x4002);
    assert_eq!(link["witnesses"].as_array().unwrap().len(), 9);
    assert_eq!(link["witnesses"][0]["pc"], 0x8010);
    assert_eq!(link["witnesses"][8]["pc"], 0x801e);
}

#[test]
fn direct_x_and_y_arguments_reach_matching_stores() {
    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    write(&mut rom, 0x8010, &[0xbd, 0x00, 0x81]);
    write(&mut rom, 0x8013, &[0xaa]);
    write(&mut rom, 0x8014, &[0x8e, 0x00, 0x40]);
    write(&mut rom, 0x8017, &[0x60]);
    write(&mut rom, 0x8020, &[0x20, 0x30, 0x80]);
    write(&mut rom, 0x8030, &[0xb9, 0x00, 0x81]);
    write(&mut rom, 0x8033, &[0xa8]);
    write(&mut rom, 0x8034, &[0x8c, 0x01, 0x40]);
    rom[offset(0x8101)] = 0x42;
    rom[offset(0x8102)] = 0x73;
    let mut tracker = Tracker::new(&rom).unwrap();

    let first = state(0x8000, 0, 1, 2, 0xfd, 7);
    let first_entry = state(0x8010, 0, 1, 2, 0xfb, 13);
    call(&mut tracker, first, first_entry);
    let first_load = state(0x8013, 0x42, 1, 2, 0xfb, 17);
    tracker
        .step(
            first_entry,
            first_load,
            Some(&record(first_entry, &[0xbd, 0x00, 0x81])),
            &[],
        )
        .unwrap();
    let first_copy = state(0x8014, 0x42, 0x42, 2, 0xfb, 19);
    tracker
        .step(
            first_load,
            first_copy,
            Some(&record(first_load, &[0xaa])),
            &[],
        )
        .unwrap();
    let first_done = state(0x8017, 0x42, 0x42, 2, 0xfb, 23);
    tracker
        .step(
            first_copy,
            first_done,
            Some(&record(first_copy, &[0x8e, 0x00, 0x40])),
            &[(1, writer(0x8014, 0x4000, 0x42, 16))],
        )
        .unwrap();

    let returned = state(0x8020, 0x42, 0x42, 2, 0xfd, 29);
    tracker
        .step(
            first_done,
            returned,
            Some(&record(first_done, &[0x60])),
            &[],
        )
        .unwrap();
    let second = returned;
    let second_entry = state(0x8030, 0x42, 0x42, 2, 0xfb, 35);
    call(&mut tracker, second, second_entry);
    let second_load = state(0x8033, 0x73, 0x42, 2, 0xfb, 39);
    tracker
        .step(
            second_entry,
            second_load,
            Some(&record(second_entry, &[0xb9, 0x00, 0x81])),
            &[],
        )
        .unwrap();
    let second_copy = state(0x8034, 0x73, 0x42, 0x73, 0xfb, 41);
    tracker
        .step(
            second_load,
            second_copy,
            Some(&record(second_load, &[0xa8])),
            &[],
        )
        .unwrap();
    let second_done = state(0x8037, 0x73, 0x42, 0x73, 0xfb, 45);
    tracker
        .step(
            second_copy,
            second_done,
            Some(&record(second_copy, &[0x8c, 0x01, 0x40])),
            &[(2, writer(0x8034, 0x4001, 0x73, 34))],
        )
        .unwrap();

    let links = tracker.finish();
    assert_eq!(links.as_array().unwrap().len(), 2);
    assert_eq!(links[0]["argument"]["register"], "x");
    assert_eq!(links[1]["argument"]["register"], "y");
}

#[test]
fn clobbers_and_unknown_opcodes_discard_lineage_even_at_the_same_value() {
    for bytes in [&[0xa2, 1][..], &[0x49, 0][..]] {
        let mut rom = source();
        write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
        write(&mut rom, 0x8010, bytes);
        let load_pc = 0x8010 + bytes.len() as u16;
        write(&mut rom, load_pc, &[0xbd, 0x00, 0x81]);
        write(&mut rom, load_pc + 3, &[0x8d, 0x00, 0x40]);
        rom[offset(0x8101)] = 0x55;
        let mut tracker = Tracker::new(&rom).unwrap();
        let before = state(0x8000, 1, 1, 0, 0xfd, 7);
        let entry = state(0x8010, 1, 1, 0, 0xfb, 13);
        call(&mut tracker, before, entry);
        let after = state(load_pc, 1, 1, 0, 0xfb, 15);
        tracker
            .step(entry, after, Some(&record(entry, bytes)), &[])
            .unwrap();
        if bytes[0] == 0xa2 {
            assert!(tracker.window.as_ref().unwrap().x.is_none());
        } else {
            assert!(tracker.window.is_none());
        }
        let loaded = state(load_pc + 3, 0x55, 1, 0, 0xfb, 19);
        tracker
            .step(
                after,
                loaded,
                Some(&record(after, &[0xbd, 0x00, 0x81])),
                &[],
            )
            .unwrap();
        let done = state(load_pc + 6, 0x55, 1, 0, 0xfb, 23);
        tracker
            .step(
                loaded,
                done,
                Some(&record(loaded, &[0x8d, 0x00, 0x40])),
                &[(0, writer(load_pc + 3, 0x4000, 0x55, 16))],
            )
            .unwrap();
        assert_eq!(tracker.finish(), json!([]));
    }
}

#[test]
fn boundaries_and_noncontiguous_state_cannot_resume_a_window() {
    for barrier in [0x00, 0x40, 0x60] {
        let mut rom = source();
        write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
        write(&mut rom, 0x8010, &[barrier]);
        let mut tracker = Tracker::new(&rom).unwrap();
        let before = state(0x8000, 1, 1, 0, 0xfd, 7);
        let entry = state(0x8010, 1, 1, 0, 0xfb, 13);
        call(&mut tracker, before, entry);
        let after = state(0x8020, 1, 1, 0, 0xfb, 17);
        tracker
            .step(entry, after, Some(&record(entry, &[barrier])), &[])
            .unwrap();
        assert!(tracker.window.is_none());
        assert_eq!(tracker.finish(), json!([]));
    }

    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    let mut tracker = Tracker::new(&rom).unwrap();
    let before = state(0x8000, 1, 1, 0, 0xfd, 7);
    let entry = state(0x8010, 1, 1, 0, 0xfb, 13);
    call(&mut tracker, before, entry);
    tracker.step(entry, entry, None, &[]).unwrap();
    assert!(tracker.window.is_none());
    assert_eq!(tracker.finish(), json!([]));

    let mut tracker = Tracker::new(&rom).unwrap();
    call(&mut tracker, before, entry);
    let mut interrupt = record(entry, &[]);
    interrupt.event = Some(DebugEvent::Interrupt);
    tracker
        .step(
            entry,
            state(0x9000, 1, 1, 0, 0xf8, 19),
            Some(&interrupt),
            &[],
        )
        .unwrap();
    assert!(tracker.window.is_none());
    assert_eq!(tracker.finish(), json!([]));

    let mut tracker = Tracker::new(&rom).unwrap();
    call(&mut tracker, before, entry);
    assert_eq!(
        tracker.step(
            state(0x8011, 1, 1, 0, 0xfb, 15),
            state(0x8012, 1, 1, 0, 0xfb, 17),
            Some(&record(state(0x8011, 1, 1, 0, 0xfb, 15), &[0xea])),
            &[]
        ),
        Err("argument_flow_noncontiguous_state")
    );
}

#[test]
fn nested_call_replaces_the_outer_window_and_does_not_restore_it() {
    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    write(&mut rom, 0x8010, &[0x20, 0x20, 0x80]);
    write(&mut rom, 0x8020, &[0x60]);
    let mut tracker = Tracker::new(&rom).unwrap();
    let first = state(0x8000, 1, 1, 0, 0xfd, 7);
    let outer = state(0x8010, 1, 1, 0, 0xfb, 13);
    call(&mut tracker, first, outer);
    let inner = state(0x8020, 1, 1, 0, 0xf9, 19);
    call(&mut tracker, outer, inner);
    let returned = state(0x8013, 1, 1, 0, 0xfb, 25);
    tracker
        .step(inner, returned, Some(&record(inner, &[0x60])), &[])
        .unwrap();
    assert!(tracker.window.is_none());
    assert_eq!(tracker.finish(), json!([]));
}

#[test]
fn sixty_four_callee_instructions_expire_and_link_overflow_errors() {
    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    for pc in 0x8010..0x8050 {
        write(&mut rom, pc, &[0xea]);
    }
    let mut tracker = Tracker::new(&rom).unwrap();
    let first = state(0x8000, 0, 0, 0, 0xfd, 7);
    let mut before = state(0x8010, 0, 0, 0, 0xfb, 13);
    call(&mut tracker, first, before);
    for _ in 0..MAX_CALLEE_INSTRUCTIONS {
        let after = state(before.pc + 1, 0, 0, 0, 0xfb, before.cycle + 2);
        tracker
            .step(before, after, Some(&record(before, &[0xea])), &[])
            .unwrap();
        before = after;
    }
    assert!(tracker.window.is_none());
    assert_eq!(tracker.finish(), json!([]));

    let mut rom = source();
    write(&mut rom, 0x8000, &[0x20, 0x10, 0x80]);
    write(&mut rom, 0x8010, &[0xbd, 0x00, 0x81]);
    write(&mut rom, 0x8013, &[0x8d, 0x00, 0x40]);
    rom[offset(0x8100)] = 0x55;
    let mut tracker = Tracker::new(&rom).unwrap();
    let call_before = state(0x8000, 0, 0, 0, 0xfd, 7);
    let entry = state(0x8010, 0, 0, 0, 0xfb, 13);
    call(&mut tracker, call_before, entry);
    let loaded = state(0x8013, 0x55, 0, 0, 0xfb, 17);
    tracker
        .step(
            entry,
            loaded,
            Some(&record(entry, &[0xbd, 0x00, 0x81])),
            &[],
        )
        .unwrap();
    let writers = (0..=MAX_LINKS)
        .map(|index| (index, writer(0x8013, 0x4000, 0x55, 10)))
        .collect::<Vec<_>>();
    assert_eq!(
        tracker.step(
            loaded,
            state(0x8016, 0x55, 0, 0, 0xfb, 21),
            Some(&record(loaded, &[0x8d, 0x00, 0x40])),
            &writers,
        ),
        Err("argument_flow_link_limit")
    );
}
