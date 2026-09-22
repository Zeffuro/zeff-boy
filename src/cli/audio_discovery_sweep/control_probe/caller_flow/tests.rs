use serde_json::{Value, json};
use zeff_emu_common::debug::{InstructionTraceRecord, TraceExecMode};

use super::*;

fn source() -> Vec<u8> {
    let mut source = vec![0; 16 + 0x4000];
    source[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 0]);
    source
}

fn offset(pc: u16) -> usize {
    16 + (usize::from(pc) - 0x8000) % 0x4000
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

fn with_carry(mut state: State, carry: bool) -> State {
    state.p = 0x24 | u8::from(carry);
    state
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

fn link(entry: State, register: &str, value: u8, event_index: u64) -> Value {
    json!({
        "entry": {"pc": entry.pc, "cpu_cycle": entry.cycle},
        "argument": {"register": register, "value": value},
        "event_index": event_index,
    })
}

fn start(tracker: &mut Tracker, entry: State) {
    let before = state(0x8000, 0, 0, 0, entry.sp.wrapping_add(2), entry.cycle - 6);
    tracker
        .step(
            before,
            entry,
            Some(&record(
                before,
                &[0x20, entry.pc as u8, (entry.pc >> 8) as u8],
            )),
            &[],
            0,
        )
        .unwrap();
}

#[test]
fn joins_exact_child_argument_after_copy_and_barrier() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let read = state(0x8102, 6, 0, 0, 0xfb, 16);
    tracker
        .step(entry, read, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let copied = state(0x8103, 6, 0, 6, 0xfb, 18);
    tracker
        .step(read, copied, Some(&record(read, &[0xa8])), &[], 0)
        .unwrap();
    let child = state(0x8200, 6, 0, 6, 0xf9, 24);
    tracker
        .step(
            copied,
            child,
            Some(&record(copied, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let after_nop = state(0x8201, 6, 0, 6, 0xf9, 26);
    let wrong_cycle = state(0x8200, 6, 0, 6, 0xf9, 23);
    tracker
        .step(
            child,
            after_nop,
            Some(&record(child, &[0xea])),
            &[
                link(wrong_cycle, "y", 6, 3),
                link(child, "x", 0, 4),
                link(child, "y", 6, 8),
            ],
            41,
        )
        .unwrap();

    let links = tracker.finish();
    assert_eq!(links.as_array().unwrap().len(), 1);
    let joined = &links[0];
    assert_eq!(joined["entry_argument_read_index"], 43);
    assert_eq!(joined["event_index"], 8);
    assert_eq!(joined["caller_entry"]["pc"], 0x8100);
    assert_eq!(joined["call"]["pc"], 0x8103);
    assert_eq!(joined["ram_read"]["address"], 0xad);
    assert_eq!(joined["witnesses"].as_array().unwrap().len(), 3);
}

#[test]
fn records_wrapping_asl_and_internal_ram_mirror() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let read = state(0x8103, 128, 0, 0, 0xfb, 17);
    tracker
        .step(
            entry,
            read,
            Some(&record(entry, &[0xad, 0xad, 0x08])),
            &[],
            0,
        )
        .unwrap();
    let shifted = with_carry(state(0x8104, 0, 0, 0, 0xfb, 19), true);
    tracker
        .step(read, shifted, Some(&record(read, &[0x0a])), &[], 0)
        .unwrap();
    let shifted_again = with_carry(state(0x8105, 0, 0, 0, 0xfb, 21), false);
    tracker
        .step(
            shifted,
            shifted_again,
            Some(&record(shifted, &[0x0a])),
            &[],
            0,
        )
        .unwrap();
    let copied = state(0x8106, 0, 0, 0, 0xfb, 23);
    tracker
        .step(
            shifted_again,
            copied,
            Some(&record(shifted_again, &[0xa8])),
            &[],
            0,
        )
        .unwrap();
    let child = state(0x8200, 0, 0, 0, 0xf9, 29);
    tracker
        .step(
            copied,
            child,
            Some(&record(copied, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let after_nop = state(0x8201, 0, 0, 0, 0xf9, 31);
    tracker
        .step(
            child,
            after_nop,
            Some(&record(child, &[0xea])),
            &[link(child, "y", 0, 3)],
            0,
        )
        .unwrap();

    let joined = &tracker.finish()[0];
    assert_eq!(joined["ram_read"]["canonical_address"], 0xad);
    assert_eq!(joined["ram_read"]["value"], 128);
    assert_eq!(joined["transforms"][0]["input"], 128);
    assert_eq!(joined["transforms"][0]["output"], 0);
    assert_eq!(joined["transforms"][0]["carry"], true);
    assert_eq!(joined["transforms"][1]["carry"], false);
}

#[test]
fn equal_value_clobber_kills_only_its_destination() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let a_loaded = state(0x8102, 4, 0, 0, 0xfb, 16);
    tracker
        .step(entry, a_loaded, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let y_loaded = state(0x8104, 4, 0, 4, 0xfb, 19);
    tracker
        .step(
            a_loaded,
            y_loaded,
            Some(&record(a_loaded, &[0xa4, 0xad])),
            &[],
            0,
        )
        .unwrap();
    let y_clobbered = state(0x8106, 4, 0, 4, 0xfb, 21);
    tracker
        .step(
            y_loaded,
            y_clobbered,
            Some(&record(y_loaded, &[0xa0, 0x04])),
            &[],
            0,
        )
        .unwrap();
    let child = state(0x8200, 4, 0, 4, 0xf9, 27);
    tracker
        .step(
            y_clobbered,
            child,
            Some(&record(y_clobbered, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let after_nop = state(0x8201, 4, 0, 4, 0xf9, 29);
    tracker
        .step(
            child,
            after_nop,
            Some(&record(child, &[0xea])),
            &[link(child, "a", 4, 1), link(child, "y", 4, 2)],
            0,
        )
        .unwrap();

    let links = tracker.finish();
    assert_eq!(links.as_array().unwrap().len(), 1);
    assert_eq!(links[0]["argument"]["register"], "a");
}

#[test]
fn excludes_mmio_and_rom_loads() {
    for bytes in [[0xad, 0x00, 0x20], [0xad, 0x00, 0x80]] {
        let rom = source();
        let mut tracker = Tracker::new(&rom).unwrap();
        let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
        start(&mut tracker, entry);
        let loaded = state(0x8103, 7, 0, 0, 0xfb, 17);
        tracker
            .step(entry, loaded, Some(&record(entry, &bytes)), &[], 0)
            .unwrap();
        let child = state(0x8200, 7, 0, 0, 0xf9, 23);
        tracker
            .step(
                loaded,
                child,
                Some(&record(loaded, &[0x20, 0x00, 0x82])),
                &[],
                0,
            )
            .unwrap();
        let after_nop = state(0x8201, 7, 0, 0, 0xf9, 25);
        tracker
            .step(
                child,
                after_nop,
                Some(&record(child, &[0xea])),
                &[link(child, "a", 7, 1)],
                0,
            )
            .unwrap();
        assert!(tracker.finish().as_array().unwrap().is_empty());
    }
}

#[test]
fn child_jsr_is_the_sixty_fourth_allowed_witness() {
    for accepted in [true, false] {
        let rom = source();
        let mut tracker = Tracker::new(&rom).unwrap();
        let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
        start(&mut tracker, entry);
        let mut before = entry;
        let loaded = state(0x8102, 1, 0, 0, 0xfb, 16);
        tracker
            .step(before, loaded, Some(&record(before, &[0xa5, 0xad])), &[], 0)
            .unwrap();
        before = loaded;
        for _ in 0..if accepted { 62 } else { 63 } {
            let after = state(
                before.pc.wrapping_add(1),
                before.a,
                before.x,
                before.y,
                before.sp,
                before.cycle + 2,
            );
            tracker
                .step(before, after, Some(&record(before, &[0xea])), &[], 0)
                .unwrap();
            before = after;
        }
        let child = state(0x8200, 1, 0, 0, 0xf9, before.cycle + 6);
        tracker
            .step(
                before,
                child,
                Some(&record(before, &[0x20, 0x00, 0x82])),
                &[],
                0,
            )
            .unwrap();
        let after_nop = state(0x8201, 1, 0, 0, 0xf9, child.cycle + 2);
        tracker
            .step(
                child,
                after_nop,
                Some(&record(child, &[0xea])),
                &[link(child, "a", 1, 1)],
                0,
            )
            .unwrap();
        assert_eq!(
            tracker.finish().as_array().unwrap().len(),
            usize::from(accepted)
        );
    }
}

#[test]
fn nested_call_and_return_do_not_restore_old_binding() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let loaded = state(0x8102, 3, 0, 0, 0xfb, 16);
    tracker
        .step(entry, loaded, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let child = state(0x8200, 3, 0, 0, 0xf9, 22);
    tracker
        .step(
            loaded,
            child,
            Some(&record(loaded, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let grandchild = state(0x8300, 3, 0, 0, 0xf7, 28);
    tracker
        .step(
            child,
            grandchild,
            Some(&record(child, &[0x20, 0x00, 0x83])),
            &[],
            0,
        )
        .unwrap();
    let returned = state(0x8203, 3, 0, 0, 0xf9, 34);
    tracker
        .step(
            grandchild,
            returned,
            Some(&record(grandchild, &[0x60])),
            &[link(child, "a", 3, 9)],
            0,
        )
        .unwrap();
    assert!(tracker.finish().as_array().unwrap().is_empty());
}

#[test]
fn unsupported_instruction_discards_pending_child_binding_before_join() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let loaded = state(0x8102, 3, 0, 0, 0xfb, 16);
    tracker
        .step(entry, loaded, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let child = state(0x8200, 3, 0, 0, 0xf9, 22);
    tracker
        .step(
            loaded,
            child,
            Some(&record(loaded, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let after_eor = state(0x8202, 0, 0, 0, 0xf9, 24);
    tracker
        .step(
            child,
            after_eor,
            Some(&record(child, &[0x49, 0x03])),
            &[link(child, "a", 3, 1)],
            0,
        )
        .unwrap();
    assert!(tracker.finish().as_array().unwrap().is_empty());
}

#[test]
fn tagged_asl_requires_the_observed_carry() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let loaded = state(0x8102, 128, 0, 0, 0xfb, 16);
    tracker
        .step(entry, loaded, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let shifted_without_carry = state(0x8103, 0, 0, 0, 0xfb, 18);
    assert_eq!(
        tracker.step(
            loaded,
            shifted_without_carry,
            Some(&record(loaded, &[0x0a])),
            &[],
            0,
        ),
        Err("caller_flow_asl_mismatch")
    );
}

#[test]
fn link_cap_returns_an_error() {
    let rom = source();
    let mut tracker = Tracker::new(&rom).unwrap();
    let entry = state(0x8100, 0, 0, 0, 0xfb, 13);
    start(&mut tracker, entry);
    let loaded = state(0x8102, 3, 0, 0, 0xfb, 16);
    tracker
        .step(entry, loaded, Some(&record(entry, &[0xa5, 0xad])), &[], 0)
        .unwrap();
    let child = state(0x8200, 3, 0, 0, 0xf9, 22);
    tracker
        .step(
            loaded,
            child,
            Some(&record(loaded, &[0x20, 0x00, 0x82])),
            &[],
            0,
        )
        .unwrap();
    let after_nop = state(0x8201, 3, 0, 0, 0xf9, 24);
    let links = vec![link(child, "a", 3, 1); MAX_LINKS + 1];
    assert_eq!(
        tracker.step(child, after_nop, Some(&record(child, &[0xea])), &links, 0,),
        Err("caller_flow_link_limit")
    );
}
