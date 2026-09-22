use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeff_emu_common::audio_trace::{AudioTraceSource, NesAudioTrace, NesTraceWrite};
use zeff_emu_common::debug::InstructionTraceRecord;
use zeff_nes_core::emulator::Emulator;

use super::control_flow::{Recorder, State};

#[path = "control_probe/argument_flow.rs"]
mod argument_flow;
#[cfg(test)]
#[path = "control_probe/argument_tests.rs"]
mod argument_tests;
#[path = "control_probe/caller_flow.rs"]
mod caller_flow;
#[cfg(test)]
#[path = "control_probe/caller_tests.rs"]
mod caller_tests;
#[path = "control_probe/index_read.rs"]
mod index_read;
#[path = "control_probe/ram_writer.rs"]
mod ram_writer;
#[path = "control_probe/record_extent.rs"]
mod record_extent;
#[path = "control_probe/selector_records.rs"]
mod selector_records;
#[cfg(test)]
#[path = "control_probe/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "control_probe/writer_tests.rs"]
mod writer_tests;

const MAX_FRAMES: u64 = 720;
const MAX_STEPS: u64 = 12_000_000;

pub(super) fn unavailable(reason: &str) -> Value {
    json!({
        "schema": "zeff-audio-runtime-control/1",
        "qualification": "observed_call_context_only",
        "status": "unavailable", "reason": reason,
        "observations": [], "entry_index_reads": [], "entry_argument_reads": [],
        "caller_argument_reads": [], "record_extents": [], "writer_path_records": [],
    })
}

pub(super) fn observe(
    source: &[u8],
    expected: &NesAudioTrace,
    row: &Value,
    inventory: &Value,
    cancel: &AtomicBool,
) -> Value {
    if cancel.load(Ordering::Relaxed) {
        return unavailable("cancelled");
    }
    match run(source, expected, row, inventory, cancel, MAX_STEPS) {
        Ok(value) if !cancel.load(Ordering::Relaxed) => value,
        Ok(_) => unavailable("cancelled"),
        Err(reason) => unavailable(reason),
    }
}

fn run(
    source: &[u8],
    expected: &NesAudioTrace,
    row: &Value,
    inventory: &Value,
    cancel: &AtomicBool,
    step_limit: u64,
) -> Result<Value, &'static str> {
    if inventory["status"] != "complete"
        || inventory["identity"]["capture"] != row["capture"]
        || !super::nes_source::source_matches_report(
            source,
            &json!({"media": inventory["identity"]["media"]}),
        )
    {
        return Err("unbound_runtime_inventory");
    }
    expected
        .validate_complete()
        .map_err(|_| "incomplete_trace")?;
    if expected.events.len() > 262_144 {
        return Err("event_limit");
    }
    let context = &row["capture"]["context"];
    let frames = context["frames_run"].as_u64().ok_or("invalid_context")?;
    if !(1..=MAX_FRAMES).contains(&frames) {
        return Err("frame_limit");
    }
    if context["system"] != "nes"
        || context["requested_frames"].as_u64() != Some(frames)
        || context["settings"]["sample_rate_hz"].as_f64() != Some(48_000.0)
    {
        return Err("invalid_context");
    }
    let p1 = schedule(&context["input"]["player_1"], frames)?;
    let p2 = schedule(&context["input"]["player_2"], frames)?;
    let targets = targets(inventory)?;
    if targets.is_empty() {
        return Err("no_runtime_writers");
    }
    let mut recorder = Recorder::new(source)?;
    let mut index_reads = index_read::Tracker::new(source)?;
    let mut argument_reads = argument_flow::Tracker::new(source)?;
    let mut caller_reads = caller_flow::Tracker::new(source)?;
    let mut boundaries = Vec::new();
    let mut emulator = Emulator::new_with_audio_trace(source, 48_000.0, 262_144)
        .map_err(|_| "unsupported_capture")?;
    if emulator.cpu_cycles() != 7 {
        return Err("unsupported_cpu_origin");
    }
    emulator.set_instruction_trace_enabled(true);
    let mut next_event = 0;
    let mut steps = 0;
    let mut float_hash = Sha256::new();
    let mut pcm_hash = Sha256::new();
    let mut sample_count = 0_u64;
    let mut samples = Vec::new();
    let mut writes = Vec::new();
    for frame in 1..=frames {
        let (buttons, dpad) = masks(&p1, frame);
        emulator.set_input(buttons, dpad);
        let (buttons, dpad) = masks(&p2, frame);
        emulator.set_input_p2(buttons, dpad);
        emulator.set_zapper_state(false, false, false, None);
        emulator.clear_frame_ready();
        let frame_start = emulator.cpu_cycles();
        let frame_limit = emulator.max_cpu_cycles_per_frame() * 2;
        while !emulator.ppu_frame_ready()
            && emulator.cpu_cycles().wrapping_sub(frame_start) < frame_limit
            && !emulator.is_cpu_suspended()
        {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled");
            }
            if steps == step_limit {
                return Err("instruction_limit");
            }
            steps += 1;
            let before = state(&emulator);
            emulator.clear_instruction_trace();
            emulator.step_instruction();
            let after = state(&emulator);
            if after.cycle <= before.cycle {
                return Err("nonprogressing_execution");
            }
            writes.clear();
            while let Some(event) = expected.events.get(next_event)
                && event.cycle < after.cycle - 7
            {
                if event.cycle < before.cycle - 7 {
                    return Err("event_interval_mismatch");
                }
                if let NesTraceWrite::Register { address, .. } = event.write
                    && let AudioTraceSource::CartridgeRom {
                        offset,
                        bit_reversed: false,
                    } = event.instruction_source
                    && targets.contains(&(event.pc, offset, address))
                {
                    writes.push((next_event, *event));
                }
                next_event += 1;
            }
            let record = emulator.instruction_trace().iter().next();
            recorder.step(before, after, record, &writes)?;
            record_boundary(&mut boundaries, before.cycle, record)?;
            index_reads.step(before, after, record, &writes)?;
            let first_link = argument_reads.links().len();
            argument_reads.step(before, after, record, &writes)?;
            caller_reads.step(
                before,
                after,
                record,
                &argument_reads.links()[first_link..],
                first_link,
            )?;
        }
        emulator.drain_audio_samples_into(&mut samples);
        if !samples.len().is_multiple_of(2) || samples.iter().any(|sample| !sample.is_finite()) {
            return Err("invalid_native_pcm");
        }
        sample_count += samples.len() as u64;
        for sample in &samples {
            float_hash.update(sample.to_le_bytes());
            pcm_hash.update(((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes());
        }
    }
    let actual = emulator
        .finish_audio_trace()
        .ok_or("missing_replay_trace")?;
    if next_event != expected.events.len() || &actual != expected {
        return Err("replay_trace_mismatch");
    }
    let float_hash = const_hex::encode(float_hash.finalize());
    let pcm_hash = const_hex::encode(pcm_hash.finalize());
    let reference = &row["validation"]["native_reference"]["evidence"];
    if sample_count == 0
        || reference["byte_len"].as_u64() != Some(sample_count * 4)
        || reference["f32_sha256"] != float_hash
        || reference["pcm"]["frames"].as_u64() != Some(sample_count / 2)
        || reference["pcm"]["pcm_sha256"] != pcm_hash
    {
        return Err("replay_pcm_mismatch");
    }
    let mut result = recorder.finish();
    if result["observed_writes"] != inventory["observed_event_count"] {
        return Err("writer_count_mismatch");
    }
    result["schema"] = json!("zeff-audio-runtime-control/1");
    result["qualification"] = json!("observed_call_context_only");
    result["status"] = json!("complete");
    result["identity"] = inventory["identity"].clone();
    result["entry_index_reads"] = index_reads.finish();
    result["entry_argument_reads"] = argument_reads.finish();
    result["caller_argument_reads"] = caller_reads.finish();
    result["record_extents"] = record_extent::analyze(source, &result, &boundaries);
    result["writer_path_records"] = selector_records::analyze(source, &result);
    result["caller_flow_contract"] = json!({
        "scope": "immediate_caller_internal_ram_reads",
        "max_instructions_including_call": 64, "max_links": 4096,
        "arithmetic": "asl_a_wrapping_u8",
        "memory_writer_provenance": "authenticated_reaching_direct_store", "parent_restore": false,
        "writer_constraint_scope": "writer_path_only", "writer_constraint_max_instructions": 64,
        "writer_constraint_max_values": 16,
    });
    result["argument_flow_contract"] = json!({
        "scope": "nearest_call_register_copies",
        "max_instructions": 64, "max_links": 4096,
        "memory_spills": false, "arithmetic_lineage": false,
        "boundary_or_unsupported_instruction": "discard_live_lineage",
    });
    result["timing"] = json!({"instruction_cycles": "cpu_cycles", "audio_event_cycles": "cpu_cycles_minus_origin", "audio_event_cpu_origin": 7});
    result["verification"] = json!({
        "full_trace_matches": true, "native_f32_sha256": float_hash,
        "native_byte_len": sample_count * 4, "pcm_sha256": pcm_hash,
        "pcm_frames": sample_count / 2, "steps": steps,
    });
    result["limits"] =
        json!({"frames": MAX_FRAMES, "steps": step_limit, "call_depth": 32, "observations": 4096});
    result["limitations"] = json!([
        "Call entries and register arguments are observed execution evidence only.",
        "Entry-index links require adjacent JSR, indexed ROM load and sound store instructions.",
        "Entry-argument links follow bounded register copies; they do not track memory spills or arithmetic lineage.",
        "Caller links join direct internal-RAM reads and wrapping shifts to observed callee arguments; they do not identify valid selectors.",
        "RAM writers and finite value constraints apply only to the witnessed writer path, not every possible selector producer.",
        "Record extents cover one observed copy loop; unexecuted writer-path candidates remain conditional and may be unreachable.",
        "These observations do not enumerate songs or establish a playback or export contract.",
    ]);
    Ok(result)
}

fn record_boundary(
    boundaries: &mut Vec<u64>,
    cycle: u64,
    record: Option<&InstructionTraceRecord>,
) -> Result<(), &'static str> {
    if record.is_none_or(|record| {
        record.event.is_some() || record.instruction_bytes().first() == Some(&0x00)
    }) {
        if boundaries.len() == 4096 {
            return Err("record_boundary_limit");
        }
        boundaries.push(cycle);
    }
    Ok(())
}

fn state(emulator: &Emulator) -> State {
    State {
        pc: emulator.cpu_pc(),
        a: emulator.cpu_a(),
        x: emulator.cpu_x(),
        y: emulator.cpu_y(),
        sp: emulator.cpu_sp(),
        p: emulator.cpu_status(),
        cycle: emulator.cpu_cycles(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    start_frame: u64,
    end_frame: u64,
    buttons: u8,
    dpad: u8,
    coleco_keypad: Option<u8>,
    reset: bool,
}

fn schedule(value: &Value, frames: u64) -> Result<Vec<Input>, &'static str> {
    if value.as_array().is_none_or(|events| events.len() > 2048) {
        return Err("invalid_input_schedule");
    }
    let inputs: Vec<Input> =
        serde_json::from_value(value.clone()).map_err(|_| "invalid_input_schedule")?;
    if inputs.iter().any(|input| {
        input.start_frame == 0
            || input.end_frame < input.start_frame
            || input.end_frame > frames
            || input.reset
            || input.coleco_keypad.is_some()
    }) {
        return Err("unsupported_input_schedule");
    }
    Ok(inputs)
}

fn masks(inputs: &[Input], frame: u64) -> (u8, u8) {
    inputs
        .iter()
        .filter(|input| (input.start_frame..=input.end_frame).contains(&frame))
        .fold((0, 0), |(buttons, dpad), input| {
            (buttons | input.buttons, dpad | input.dpad)
        })
}

fn targets(inventory: &Value) -> Result<BTreeSet<(u32, u64, u16)>, &'static str> {
    let sites = inventory["sites"]
        .as_array()
        .ok_or("invalid_runtime_inventory")?;
    if sites.len() > 256 {
        return Err("site_limit");
    }
    let mut targets = BTreeSet::new();
    for site in sites {
        let pc = site["pc"]
            .as_u64()
            .and_then(|pc| u32::try_from(pc).ok())
            .ok_or("invalid_writer")?;
        let offset = site["instruction_source"]["offset"]
            .as_u64()
            .ok_or("invalid_writer")?;
        let registers = site["registers"].as_array().ok_or("invalid_writer")?;
        if registers.len() > 22 {
            return Err("invalid_writer");
        }
        for register in registers {
            let address = register["address"]
                .as_u64()
                .and_then(|address| u16::try_from(address).ok())
                .ok_or("invalid_writer")?;
            targets.insert((pc, offset, address));
        }
    }
    Ok(targets)
}
