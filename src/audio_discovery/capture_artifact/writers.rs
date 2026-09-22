use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::*;

use super::NativeTrace;

const MAX_SITES: usize = 1024;
const MAX_REPORTED_SITES: usize = 32;
const MAX_DESTINATIONS: usize = 32;

struct Write {
    kind: &'static str,
    address: u32,
    origin: Origin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Instruction,
    CpuInterrupt,
    GeneralDma,
    SoundDma,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::Instruction => "instruction",
            Self::CpuInterrupt => "cpu_interrupt",
            Self::GeneralDma => "general_dma",
            Self::SoundDma => "sound_dma",
        }
    }
}

enum Classification {
    Write(Write),
    Excluded(&'static str),
}

pub(super) fn summarize(trace: &NativeTrace, cancel: &AtomicBool) -> Result<Value> {
    match trace {
        NativeTrace::Sn(trace) => collect(trace, sn, cancel),
        NativeTrace::GameBoy(trace) => collect(trace, gb, cancel),
        NativeTrace::Nes(trace) => collect(trace, nes, cancel),
        NativeTrace::Huc6280(trace) => collect(trace.as_ref(), huc, cancel),
        NativeTrace::WonderSwan(trace) => collect(trace, ws, cancel),
    }
}

fn write(kind: &'static str, address: impl Into<u32>, origin: Origin) -> Classification {
    Classification::Write(Write {
        kind,
        address: address.into(),
        origin,
    })
}

fn sn(event: &AudioTraceWrite) -> Classification {
    match *event {
        AudioTraceWrite::Sn76489 { port, .. } => write("sn76489_port", port, Origin::Instruction),
        AudioTraceWrite::GameGearStereo { port, .. } => {
            write("gg_stereo", port, Origin::Instruction)
        }
    }
}

fn gb(event: &GameBoyTraceWrite) -> Classification {
    match *event {
        GameBoyTraceWrite::Register {
            address, origin, ..
        } => write("gb_register", address, gb_origin(origin)),
        GameBoyTraceWrite::WaveRam {
            address,
            applied_index: Some(_),
            origin,
            ..
        } => write("gb_wave_ram", address, gb_origin(origin)),
        GameBoyTraceWrite::WaveRam {
            applied_index: None,
            ..
        } => Classification::Excluded("gb_blocked_wave_ram"),
        GameBoyTraceWrite::DividerReset { .. } => Classification::Excluded("gb_divider_reset"),
        GameBoyTraceWrite::SequencerClock { .. } => Classification::Excluded("gb_sequencer_clock"),
        GameBoyTraceWrite::Stop { .. } => Classification::Excluded("gb_stop"),
        GameBoyTraceWrite::SpeedSwitch { .. } => Classification::Excluded("gb_speed_switch"),
        GameBoyTraceWrite::SpeedSwitchDelay { .. } => {
            Classification::Excluded("gb_speed_switch_delay")
        }
        GameBoyTraceWrite::NativeBatch { .. } => Classification::Excluded("gb_native_batch"),
        GameBoyTraceWrite::NativeDividerPhase { .. } => {
            Classification::Excluded("gb_divider_phase")
        }
        GameBoyTraceWrite::PcmDrain { .. } => Classification::Excluded("gb_pcm_drain"),
        GameBoyTraceWrite::NativeOutputChange { .. } => {
            Classification::Excluded("gb_output_change")
        }
    }
}

fn nes(event: &NesTraceWrite) -> Classification {
    match *event {
        NesTraceWrite::Register { address, .. } => {
            write("nes_register", address, Origin::Instruction)
        }
        NesTraceWrite::StatusRead { .. } => Classification::Excluded("nes_status_read"),
        NesTraceWrite::DmcFetch { .. } => Classification::Excluded("nes_dmc_fetch"),
    }
}

fn huc(event: &Huc6280TraceWrite) -> Classification {
    write(
        "huc6280_register",
        event.physical_address,
        Origin::Instruction,
    )
}

fn ws(event: &WonderSwanTraceWrite) -> Classification {
    match *event {
        WonderSwanTraceWrite::Register { port, origin, .. } => {
            write("ws_register", port, ws_origin(origin))
        }
        WonderSwanTraceWrite::WaveRam {
            address, origin, ..
        } => write("ws_wave_ram", address, ws_origin(origin)),
    }
}

fn gb_origin(origin: GameBoyTraceOrigin) -> Origin {
    match origin {
        GameBoyTraceOrigin::Cpu => Origin::Instruction,
        GameBoyTraceOrigin::CpuInterrupt => Origin::CpuInterrupt,
    }
}

fn ws_origin(origin: WonderSwanTraceOrigin) -> Origin {
    match origin {
        WonderSwanTraceOrigin::Cpu => Origin::Instruction,
        WonderSwanTraceOrigin::CpuInterrupt => Origin::CpuInterrupt,
        WonderSwanTraceOrigin::GeneralDma => Origin::GeneralDma,
        WonderSwanTraceOrigin::SoundDma => Origin::SoundDma,
    }
}

fn source_kind(source: AudioTraceSource) -> &'static str {
    match source {
        AudioTraceSource::CartridgeRom { .. } => "cartridge_rom",
        AudioTraceSource::BootRom { .. } => "boot_rom",
        AudioTraceSource::WorkRam { .. } => "work_ram",
        AudioTraceSource::CartridgeRam { .. } => "cartridge_ram",
        AudioTraceSource::Unknown => "unknown",
        AudioTraceSource::Unmapped => "unmapped",
    }
}

#[derive(Default)]
struct Site {
    writes: usize,
    later_writes: usize,
    first_cycle: u64,
    last_cycle: u64,
    destinations: BTreeSet<(&'static str, u32)>,
    destinations_truncated: bool,
}

fn collect<C, W>(
    trace: &ChipAudioTrace<C, W>,
    classify: impl Fn(&W) -> Classification,
    cancel: &AtomicBool,
) -> Result<Value> {
    ensure!(!cancel.load(Ordering::Relaxed), "writer evidence cancelled");
    ensure!(
        trace.cycle_hz > 0 && trace.cycle_hz_denominator > 0,
        "invalid writer evidence clock"
    );
    let cutoff = u64::from(trace.cycle_hz).div_ceil(u64::from(trace.cycle_hz_denominator));
    let mut sites = BTreeMap::<(u32, String), Site>::new();
    let mut excluded = BTreeMap::<&'static str, usize>::new();
    let mut kinds = BTreeMap::<&'static str, usize>::new();
    let mut total = 0usize;
    let mut later = 0usize;
    let mut unattributed = 0usize;
    let mut unranked = BTreeMap::<(&'static str, &'static str), (usize, usize)>::new();
    let mut omitted = 0usize;
    let mut eligible = 0usize;
    let mut eligible_later = 0usize;
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "writer evidence cancelled");
        let write = match classify(&event.write) {
            Classification::Write(write) => write,
            Classification::Excluded(reason) => {
                *excluded.entry(reason).or_default() += 1;
                continue;
            }
        };
        total += 1;
        later += usize::from(event.cycle >= cutoff);
        *kinds.entry(write.kind).or_default() += 1;
        if write.origin != Origin::Instruction
            || matches!(
                event.instruction_source,
                AudioTraceSource::Unknown | AudioTraceSource::Unmapped
            )
        {
            unattributed += 1;
            let counts = unranked
                .entry((write.origin.label(), source_kind(event.instruction_source)))
                .or_default();
            counts.0 += 1;
            counts.1 += usize::from(event.cycle >= cutoff);
            continue;
        }
        eligible += 1;
        eligible_later += usize::from(event.cycle >= cutoff);
        let key = (event.pc, serde_json::to_string(&event.instruction_source)?);
        if !sites.contains_key(&key) && sites.len() == MAX_SITES {
            omitted += 1;
            continue;
        }
        let site = sites.entry(key).or_insert_with(|| Site {
            first_cycle: event.cycle,
            ..Default::default()
        });
        site.writes += 1;
        site.later_writes += usize::from(event.cycle >= cutoff);
        site.last_cycle = event.cycle;
        let destination = (write.kind, write.address);
        if site.destinations.len() < MAX_DESTINATIONS || site.destinations.contains(&destination) {
            site.destinations.insert(destination);
        } else {
            site.destinations_truncated = true;
        }
    }
    let tracked_sites = sites.len();
    let mut ranked = sites.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_key, left), (right_key, right)| {
        right
            .later_writes
            .cmp(&left.later_writes)
            .then_with(|| right.writes.cmp(&left.writes))
            .then_with(|| left_key.cmp(right_key))
    });
    let mut rows = Vec::new();
    let mut reported = 0usize;
    let mut reported_later = 0usize;
    for ((pc, source), site) in ranked.into_iter().take(MAX_REPORTED_SITES) {
        reported += site.writes;
        reported_later += site.later_writes;
        rows.push(json!({
            "pc": pc, "instruction_source": serde_json::from_str::<Value>(&source)?,
            "write_events": site.writes, "after_first_second_write_events": site.later_writes,
            "first_cycle": site.first_cycle, "last_cycle": site.last_cycle,
            "destinations": site.destinations.into_iter().map(|(kind,address)| json!({"kind":kind,"address":address})).collect::<Vec<_>>(),
            "destinations_truncated": site.destinations_truncated,
        }));
    }
    Ok(json!({
        "schema": "zeff-audio-writer-evidence/1",
        "clock_hz_numerator": trace.cycle_hz, "clock_hz_denominator": trace.cycle_hz_denominator,
        "end_cycle": trace.end_cycle, "first_second_boundary_cycle": cutoff,
        "total_events": trace.events.len(), "write_events": total,
        "after_first_second_write_events": later, "write_kinds": kinds, "excluded_events": excluded,
        "unattributed_write_events": unattributed, "instruction_write_events": eligible,
        "unranked_attribution": unranked.into_iter().map(|((origin,source),(writes,later))|
            json!({"origin":origin,"source_kind":source,"write_events":writes,"after_first_second_write_events":later})).collect::<Vec<_>>(),
        "after_first_second_instruction_write_events": eligible_later,
        "tracked_sites": tracked_sites, "site_limit": MAX_SITES,
        "untracked_site_write_events": omitted, "site_inventory_truncated": omitted > 0,
        "reported_sites": rows, "reported_write_events": reported,
        "unreported_tracked_site_write_events": eligible - omitted - reported,
        "reported_after_first_second_write_events": reported_later,
        "reported_site_limit": MAX_REPORTED_SITES,
        "ranking": "after_first_second_writes_then_all_writes_then_pc_and_source",
        "limitations": [
            "Ranks observed instruction write sites, not function boundaries or identified drivers. The first second is a fixed observation window, not a detected initialization phase.",
            "Instruction sources identify writer code, not sequence or sample origins. Non-instruction and unknown sources do not enter the ranking.",
            "Write counts do not measure audibility. Event-clock cycles have no inferred mapping to input steps or PCM frames. A truncated inventory may omit more frequent sites.",
        ],
    }))
}

#[cfg(test)]
mod tests;
