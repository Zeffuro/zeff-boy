use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

use super::{Emulator, MAX_BUFFERED_SAMPLES, MAX_CABLE_EVENTS, Schedule};
use crate::hardware::bus::DebugTraceMode;

pub(super) fn validate(machine: &Emulator) -> Result<()> {
    ensure!(
        !machine.is_cpu_suspended() && machine.last_trap().is_none(),
        "paired machine stopped or trapped"
    );
    validate_controls(machine)?;
    ensure!(
        machine.apu_debug_snapshot().buffered_samples <= MAX_BUFFERED_SAMPLES,
        "paired audio buffer budget exhausted"
    );
    ensure!(
        machine.uart_debug_snapshot().completed_tx_count <= MAX_CABLE_EVENTS,
        "paired UART queue overflow"
    );
    Ok(())
}

pub(super) fn validate_controls(machine: &Emulator) -> Result<()> {
    ensure!(
        !machine.debug.break_on_next
            && machine.debug.iter_breakpoints().next().is_none()
            && machine.debug.iter_one_shot_breakpoints().next().is_none()
            && machine
                .debug
                .iter_breakpoint_hit_conditions()
                .next()
                .is_none()
            && machine.debug.watchpoints.is_empty()
            && machine.debug.iter_event_breakpoints().next().is_none()
            && machine.debug.hit_breakpoint.is_none()
            && machine.debug.hit_watchpoint.is_none()
            && machine.debug.hit_event.is_none()
            && !machine.opcode_log.enabled
            && machine.opcode_log.recent(1).is_empty()
            && !machine.instruction_trace.is_enabled()
            && machine.instruction_trace.is_empty()
            && machine.bus.debug_trace_mode == DebugTraceMode::None
            && !machine.bus.audio_trace.is_active(),
        "paired link excludes debugger and traces"
    );
    let apu = machine.apu_debug_snapshot();
    ensure!(
        apu.sample_rate == 48_000
            && apu.sample_generation_enabled
            && apu.channel_mutes == [false; 4],
        "paired link requires full 48000 Hz audio"
    );
    Ok(())
}

pub(super) fn machine_hashes(machines: [&Emulator; 2]) -> Result<[[u8; 32]; 2]> {
    Ok([machine_hash(machines[0])?, machine_hash(machines[1])?])
}

fn machine_hash(machine: &Emulator) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"zeff-ws-pair-machine-v1\0");
    hash.update(machine.encode_state()?);
    machine.cpu.hash_rollback_runtime(&mut hash);
    machine.bus.apu.hash_rollback_runtime(&mut hash);
    machine.bus.hash_rollback_service(&mut hash);
    Ok(hash.finalize().into())
}

pub(super) fn pair_hash(machines: [&Emulator; 2], schedule: &Schedule) -> Result<[u8; 32]> {
    ensure!(
        schedule.events.len() <= MAX_CABLE_EVENTS,
        "paired snapshot cable queue overflow"
    );
    ensure!(
        schedule
            .events
            .iter()
            .all(|event| event.sender < 2 && matches!(event.baud_bps, 9600 | 38400)),
        "invalid paired cable event"
    );
    ensure!(
        schedule.events.windows(2).all(|events| (
            events[0].tick,
            events[0].sender,
            events[0].generation
        ) <= (
            events[1].tick,
            events[1].sender,
            events[1].generation
        )),
        "unordered paired cable events"
    );
    let mut hash = Sha256::new();
    hash.update(b"zeff-ws-pair-checkpoint-v1\0");
    for machine in machine_hashes(machines)? {
        hash.update(machine);
    }
    for epoch in schedule.epochs {
        hash.update(epoch.to_le_bytes());
    }
    hash.update(schedule.frame.to_le_bytes());
    hash.update((schedule.events.len() as u64).to_le_bytes());
    for event in &schedule.events {
        hash.update(event.tick.to_le_bytes());
        hash.update([event.sender as u8, event.byte]);
        hash.update(event.generation.to_le_bytes());
        hash.update(event.baud_bps.to_le_bytes());
    }
    Ok(hash.finalize().into())
}
