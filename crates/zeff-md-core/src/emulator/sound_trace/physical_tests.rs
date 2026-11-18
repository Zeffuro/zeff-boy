use super::*;
use crate::sound_trace::{
    SoundTraceContinuation,
    physical::{Event, Fact},
};
use zeff_emu_common::debug::InstructionTraceStore;

fn fixture() -> (Emulator, usize) {
    let sound = [0x3e, 0x5a, 0x32, 0, 0x10, 0x00, 0x76];
    let mut main = Vec::new();
    let mut byte = |address: u32, value: u8| {
        main.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ])
    };
    byte(0xa11100, 1);
    byte(0xa11200, 1);
    for (offset, value) in sound.into_iter().enumerate() {
        byte(0xa00000 + offset as u32, value);
    }
    for _ in 0..4 {
        byte(0xa11200, 0);
    }
    byte(0xa11200, 1);
    byte(0xa11100, 0);
    let held_steps = main.len() / 4 - 2;
    main.extend([0x60fe, 0x4e71]);
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in main.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut core = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
        TimingModel::Precise,
    )
    .unwrap();
    for _ in 0..held_steps {
        core.step_instruction().unwrap();
    }
    assert!(core.sound_bus_requested() && core.sound_reset_asserted());
    let pins = core.bus.timed_port_sound_phase().unwrap().0;
    assert!(
        !pins.bus_ack
            && !pins.mreq
            && !pins.read
            && !pins.write
            && !pins.m1
            && !pins.iorq
            && !pins.refresh
    );
    assert!(pins.address.is_none() && pins.data.is_none());
    assert_eq!(&core.bus.active_sound_memory().ram[..sound.len()], &sound);
    assert_eq!(core.sound_cpu().cycles(), 4);
    assert!(!core.bus.active_sound_state().unwrap().pending());
    assert!(core.bus.owned_fault().is_none());
    (core, 2)
}
fn events(trace: &mut SoundTraceContinuation, output: &mut InstructionTraceStore) -> Vec<Event> {
    trace
        .attach(output)
        .physical_journal()
        .events()
        .iter()
        .copied()
        .collect()
}
fn step(
    core: &mut Emulator,
    trace: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
) {
    core.step_instruction_with(&mut Direct, &mut trace.attach(output))
        .unwrap();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_opaque_pair_restores_branch_and_public_admission_stays_closed() {
    let (mut core, count) = fixture();
    let mut trace = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::default();
    output.set_enabled(true);
    let original = core.clone();
    assert!(core.passive_sound_trace_rejection().is_some());
    assert!(
        core.step_instruction_sound_traced(&mut trace.attach(&mut output))
            .is_err()
    );
    assert_eq!(core, original);
    assert!(events(&mut trace, &mut output).is_empty());
    for _ in 0..count + 16 {
        step(&mut core, &mut trace, &mut output);
    }
    let before = events(&mut trace, &mut output);
    let before_rows: Vec<_> = output.iter().copied().collect();
    assert!(
        before
            .iter()
            .any(|e| matches!(e.fact, Fact::Retired(_) | Fact::ClassifiedRetired(_)))
    );
    let machine = core.clone();
    let raw = core.encode_state().unwrap();
    let frontend = core.encode_frontend_state().unwrap();
    let owned = core.encode_debugger_checkpoint().unwrap();
    let fork = trace.fork(&mut core, &mut output);
    for _ in 0..32 {
        step(&mut core, &mut trace, &mut output);
    }
    let child = events(&mut trace, &mut output);
    let child_rows: Vec<_> = output.iter().copied().collect();
    assert_ne!(child, before);
    fork.restore(&mut core, &mut trace, &mut output);
    assert_eq!(core, machine);
    assert_eq!(core.encode_state().unwrap(), raw);
    assert_eq!(core.encode_frontend_state().unwrap(), frontend);
    assert_eq!(core.encode_debugger_checkpoint().unwrap(), owned);
    assert_eq!(events(&mut trace, &mut output), before);
    assert_eq!(output.iter().copied().collect::<Vec<_>>(), before_rows);
    for _ in 0..32 {
        step(&mut core, &mut trace, &mut output);
    }
    assert_eq!(events(&mut trace, &mut output), child);
    assert_eq!(output.iter().copied().collect::<Vec<_>>(), child_rows);
    assert!(
        output
            .iter()
            .any(|row| row.instruction_bytes() == [0x3e, 0x5a])
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_disable_reenable_never_adopts_old_occurrences() {
    let (mut core, count) = fixture();
    let mut trace = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::default();
    output.set_enabled(true);
    for _ in 0..count + 4 {
        step(&mut core, &mut trace, &mut output);
    }
    let before = events(&mut trace, &mut output);
    let old = before.iter().rev().find_map(|e| e.occurrence).unwrap();
    trace.disable(&mut output);
    for _ in 0..4 {
        step(&mut core, &mut trace, &mut output);
    }
    assert_eq!(events(&mut trace, &mut output), before);
    output.set_enabled(true);
    for _ in 0..4 {
        step(&mut core, &mut trace, &mut output);
    }
    let after = events(&mut trace, &mut output);
    let new = after.iter().rev().find_map(|e| e.occurrence).unwrap();
    assert_ne!(old.generation, new.generation);
    assert!(after[before.len()..].iter().all(|e| {
        e.occurrence
            .is_none_or(|id| id.generation == new.generation)
    }));
    assert!(after.iter().all(|e| e.frame.is_none()));
    assert!(core.passive_sound_trace_rejection().is_some());
}
