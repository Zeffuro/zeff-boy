use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_audio_discovery::huge::{
    discovery,
    isolation::{self, IsolatedRom},
};
use zeff_emu_common::debug::BusAccessEvent;
use zeff_gb_core::{
    emulator::Emulator,
    hardware::types::{ImeState, hardware_mode::HardwareMode},
};

#[path = "huge_validation/access.rs"]
mod access;
#[path = "huge_validation/recurrence.rs"]
mod recurrence;
pub use recurrence::required_frames;

struct Execution {
    pcm: Vec<f32>,
    writes: Vec<(u64, u32, u32)>,
    updates: Vec<u64>,
    cycles: u64,
    accesses: Value,
    recurrence: Value,
}

fn run(
    bytes: &[u8],
    plan: &IsolatedRom,
    song: &discovery::BoundSong,
    frames: u32,
    sample_rate: u32,
    cancel: &AtomicBool,
) -> Result<Execution> {
    let mut emulator = Emulator::new(bytes, sample_rate)?;
    ensure!(
        emulator.hardware_mode() == HardwareMode::DMG,
        "closure requires DMG timing"
    );
    let update = song.evidence.update_address;
    let mut audit = access::AccessAudit::new(plan);
    let mut recurrence = recurrence::RecurrenceAudit::new(plan, update, song.song.loop_ticks)?;
    let mut writes = Vec::new();
    let mut updates = Vec::new();
    let mut init_count = 0;
    let target = u64::from(frames) * 70_224;
    let mut steps = 0;
    while emulator.cpu_cycles() < target {
        ensure!(!cancel.load(Ordering::Relaxed), "hUGE validation cancelled");
        ensure!(steps < target / 4 + 1, "instruction limit reached");
        steps += 1;
        let pc = emulator.cpu_pc();
        audit.check_pc(pc)?;
        if pc == plan.driver_start {
            init_count += 1;
            ensure!(init_count == 1, "driver initialized more than once");
            ensure!(
                u16::from_be_bytes([emulator.cpu_h(), emulator.cpu_l()]) == plan.descriptor,
                "init descriptor differs"
            );
            ensure!(
                emulator.cpu_ime() == ImeState::Disabled && emulator.ie_reg() == 0,
                "init interrupt contract differs"
            );
            ensure!(
                emulator.cpu_peek8(0xff26) & 0x80 != 0
                    && emulator.cpu_peek8(0xff25) == 0xff
                    && emulator.cpu_peek8(0xff24) == 0x77,
                "APU initialization differs"
            );
            ensure!(
                return_address(&emulator) == plan.init_call + 3,
                "unexpected init caller"
            );
        }
        if pc == update {
            ensure!(init_count == 1, "update ran before initialization");
            ensure!(
                emulator.cpu_ime() == ImeState::Disabled
                    && emulator.ie_reg() == 1
                    && emulator.if_reg() & 1 == 0,
                "update interrupt contract differs"
            );
            ensure!(
                return_address(&emulator) == plan.update_call + 3,
                "unexpected update caller"
            );
            recurrence.on_update(&emulator)?;
            updates.push(emulator.cpu_cycles());
        }
        let mut failure = None;
        let (_, _, _, cycles) = emulator.step_instruction_with_accesses(|event| {
            if failure.is_none() {
                failure = audit
                    .observe(event)
                    .and_then(|()| recurrence.observe(event))
                    .err();
            }
            if let BusAccessEvent::Write {
                at: Some(at),
                addr,
                written_value,
                ..
            } = event
                && ((0xff10..=0xff26).contains(&addr) || (0xff30..=0xff3f).contains(&addr))
            {
                writes.push((at.get(), addr, written_value));
            }
        });
        if let Some(error) = failure {
            return Err(error.context(format!("at PC {pc:#06x}")));
        }
        ensure!(
            cycles > 0 && !emulator.is_cpu_suspended(),
            "execution stopped"
        );
    }
    ensure!(
        init_count == 1 && updates.len() == frames as usize,
        "missing init/update calls"
    );
    ensure!(
        updates.windows(2).all(|pair| pair[1] - pair[0] == 70_224),
        "non-periodic update calls"
    );
    let pcm = emulator.drain_audio_samples();
    ensure!(
        !pcm.is_empty() && pcm.iter().all(|x| x.is_finite()) && pcm.iter().any(|x| *x != 0.0),
        "invalid or silent PCM"
    );
    Ok(Execution {
        pcm,
        writes,
        updates,
        cycles: emulator.cpu_cycles(),
        accesses: audit.report(),
        recurrence: recurrence.report()?,
    })
}

fn return_address(emulator: &Emulator) -> u16 {
    let sp = emulator.cpu_sp();
    u16::from_le_bytes([
        emulator.cpu_peek8(sp),
        emulator.cpu_peek8(sp.wrapping_add(1)),
    ])
}

fn summary(execution: &Execution) -> Result<Value> {
    let pcm: Vec<u8> = execution.pcm.iter().flat_map(|x| x.to_le_bytes()).collect();
    let pcm_s16: Vec<u8> = execution
        .pcm
        .iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes())
        .collect();
    Ok(json!({
        "pcm_frames":execution.pcm.len()/2,
        "pcm_f32_sha256":zeff_firmware::sha256_hex(&pcm),
        "pcm_s16_sha256":zeff_firmware::sha256_hex(&pcm_s16),
        "sound_writes":execution.writes.len(),
        "sound_writes_sha256":zeff_firmware::sha256_hex(&serde_json::to_vec(&execution.writes)?),
        "update_count":execution.updates.len(),
        "first_update_cycle":execution.updates.first(),
        "update_period_cycles":70224,
        "cycles":execution.cycles,
        "accesses":execution.accesses,
        "recurrence":execution.recurrence,
    }))
}

pub struct Validated {
    pub report: Value,
    pub pcm: Vec<f32>,
    pub isolated: Vec<u8>,
}

pub fn validate(
    bytes: &[u8],
    descriptor: u16,
    frames: u32,
    cancel: &AtomicBool,
) -> Result<Validated> {
    validate_at_rate(bytes, descriptor, frames, 48_000, cancel)
}

pub fn validate_at_rate(
    bytes: &[u8],
    descriptor: u16,
    frames: u32,
    sample_rate: u32,
    cancel: &AtomicBool,
) -> Result<Validated> {
    ensure!(
        (8_000..=192_000).contains(&sample_rate),
        "sample rate must be 8000..192000 Hz"
    );
    ensure!((4..=1024).contains(&frames), "frame budget must be 4..1024");
    ensure!(!cancel.load(Ordering::Relaxed), "hUGE validation cancelled");
    let report = discovery::discover(bytes, Default::default(), cancel)
        .map_err(|stop| anyhow::anyhow!("discovery stopped: {stop:?}"))?;
    let songs: Vec<_> = report
        .bound
        .iter()
        .filter(|s| s.song.descriptor.offset == u32::from(descriptor))
        .collect();
    ensure!(songs.len() == 1, "descriptor must bind exactly one driver");
    let song = songs[0];
    ensure!(
        frames >= required_frames(song.song.loop_ticks)?,
        "proof must cover warmup and two complete control periods"
    );
    let zero = isolation::build(bytes, song, 0, cancel)?;
    let poison = isolation::build(bytes, song, 0xff, cancel)?;
    for span in &zero.bootstrap_spans {
        let range = span.offset as usize..(span.offset + span.byte_len) as usize;
        ensure!(
            bytes[range.clone()] == zero.bytes[range],
            "original player does not match the proven bootstrap contract"
        );
    }
    let original = run(bytes, &zero, song, frames, sample_rate, cancel)?;
    let mut layouts = Vec::new();
    for (fill, plan) in [(0u8, &zero), (0xff, &poison)] {
        let isolated = run(&plan.bytes, plan, song, frames, sample_rate, cancel)?;
        ensure!(
            original.writes == isolated.writes
                && original.updates == isolated.updates
                && original.cycles == isolated.cycles,
            "isolated execution timing differs"
        );
        ensure!(
            original.recurrence == isolated.recurrence && original.accesses == isolated.accesses,
            "isolated control state or CPU accesses differ"
        );
        ensure!(
            original.pcm.len() == isolated.pcm.len()
                && original
                    .pcm
                    .iter()
                    .zip(&isolated.pcm)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
            "isolated PCM differs"
        );
        layouts.push(json!({"fill":fill,"rom_sha256":zeff_firmware::sha256_hex(&plan.bytes),"execution":summary(&isolated)?}));
    }
    let report = json!({
        "schema":"zeff-huge-closure/2", "passed":true,
        "source_sha256":zeff_firmware::sha256_hex(bytes),
        "descriptor":descriptor,"frames":frames,"sample_rate":sample_rate,"isolation":zero,
        "original":summary(&original)?,"isolated":layouts,
        "limitation":"Static catalog eligibility is separate from this proof of CPU-access closure and exact execution/PCM equivalence for this bounded DMG selection, sample rate and run; no all-songs, seamless-loop, external-hardware or GBS qualification."
    });
    ensure!(!cancel.load(Ordering::Relaxed), "hUGE validation cancelled");
    Ok(Validated {
        report,
        pcm: original.pcm,
        isolated: zero.bytes,
    })
}
