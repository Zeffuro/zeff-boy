use super::*;
use crate::sound_trace::{
    SoundTraceContinuation,
    physical::{Event, Fact},
};
use zeff_emu_common::debug::{
    InstructionTraceRecord, InstructionTraceStore, TraceBoundaryKind, TraceFrameKnowledge,
    TraceObservationEnd, TraceObservationStart,
};

mod lifecycle;
mod rows;
mod vdp;

fn machine(
    region: Region,
    sound: &[u8],
    tail: &[u16],
    release: bool,
    bank: bool,
) -> (Emulator, usize) {
    let mut main = Vec::new();
    let mut byte = |address: u32, value: u8| {
        main.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ])
    };
    byte(0xff8000, 0x22);
    byte(0xa11100, 1);
    byte(0xa11200, 1);
    if bank {
        for _ in 0..9 {
            byte(0xa06000, 1);
        }
    }
    for (offset, &value) in sound.iter().enumerate() {
        byte(0xa00000 + offset as u32, value);
    }
    byte(0xa01000, 0x5a);
    for _ in 0..4 {
        byte(0xa11200, 0);
    }
    byte(0xa11200, 1);
    if release {
        byte(0xa11100, 0);
    }
    let release_steps = 1 + usize::from(release);
    let held_steps = main.len() / 4 - release_steps;
    let count = if release {
        release_steps
    } else {
        release_steps + 4
    };
    if !release {
        main.extend([0x4e71; 4]);
    }
    main.extend(tail);
    main.extend([0x60fe, 0x4e71]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in main.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut core = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
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
    assert_eq!(&core.bus.active_sound_memory().ram[..sound.len()], sound);
    assert_eq!(core.bus.active_sound_memory().ram[0x1000], 0x5a);
    assert_eq!(core.sound_cpu().cycles(), 4);
    assert!(!core.bus.active_sound_state().unwrap().pending());
    assert!(core.bus.owned_fault().is_none());
    (core, count)
}
fn store() -> InstructionTraceStore {
    let mut store = InstructionTraceStore::default();
    store.set_enabled(true);
    store
}
fn step(
    core: &mut Emulator,
    trace: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
) -> Result<MachineStep, MachineFault> {
    core.step_instruction_with(&mut Direct, &mut trace.attach(output))
}
fn upload(
    core: &mut Emulator,
    trace: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
    count: usize,
) {
    for _ in 0..count {
        step(core, trace, output).unwrap();
    }
}
fn rows(output: &InstructionTraceStore) -> Vec<InstructionTraceRecord> {
    output.iter().copied().collect()
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
fn port(address: u32, value: u8) -> DebugPortWrite {
    DebugPortWrite::Byte { address, value }
}
fn control(value: u16) -> DebugPortWrite {
    DebugPortWrite::Word {
        address: 0xc00004,
        value,
    }
}
fn reject_late_interlace(
    core: &mut Emulator,
    prefix: &[DebugPortWrite],
    trace: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
) {
    let mut writes = prefix.to_vec();
    writes.push(control(0x8c02));
    writes.extend([control(0x8f02); 64]);
    let mut ordinary = core.clone();
    let failure = ordinary.debug_write_ports(&writes).unwrap_err();
    assert!(matches!(
        &failure,
        DebugPortError::Execution { index, fault }
            if *index >= prefix.len()
                && format!("{fault:?}").contains("unsupported VDP sync/interlace/address profile")
    ));
    assert_eq!(
        core.debug_write_ports_captured(&writes, &mut trace.attach(output)),
        Err(failure)
    );
    exact(core, &ordinary);
}
fn exact(actual: &Emulator, ordinary: &Emulator) {
    assert_eq!(actual, ordinary);
    assert_eq!(
        actual.encode_state().unwrap(),
        ordinary.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        ordinary.encode_frontend_state().unwrap()
    );
    assert_eq!(
        actual.encode_debugger_checkpoint().unwrap(),
        ordinary.encode_debugger_checkpoint().unwrap()
    );
}
