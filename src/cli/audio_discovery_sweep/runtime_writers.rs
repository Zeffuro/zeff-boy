use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceSource, NesAudioTrace, NesTraceWrite};

use super::nes_source::{Nrom, identity, source_matches_report};

const SCHEMA: &str = "zeff-audio-runtime-writers/1";
const QUALIFICATION: &str = "observed_instruction_sites_only";
const MAX_SITES: usize = 256;
const MAX_EVENTS: usize = 262_144;

pub(super) fn unavailable(reason: &str) -> Value {
    json!({
        "schema": SCHEMA,
        "qualification": QUALIFICATION,
        "status": "unavailable",
        "reason": reason,
        "sites": [],
    })
}

pub(super) fn summarize(
    evidence: &Value,
    row: &Value,
    source: &[u8],
    trace: &NesAudioTrace,
    cancel: &AtomicBool,
) -> Value {
    if cancel.load(Ordering::Relaxed) {
        return unavailable("cancelled");
    }
    if !super::evidence::binding_matches(evidence, row) {
        return unavailable("unbound_capture_or_static_evidence");
    }
    let report = &evidence["report"];
    if report.pointer("/media/system") != Some(&json!("nes"))
        || report.pointer("/status/kind") != Some(&json!("complete"))
    {
        return unavailable("incomplete_or_non_nes_evidence");
    }
    if !source_matches_report(source, report) {
        return unavailable("source_identity_mismatch");
    }
    let Some(nrom) = Nrom::parse(source) else {
        return unavailable("unsupported_nrom_header");
    };
    if trace.events.len() > MAX_EVENTS || trace.validate_complete().is_err() {
        return unavailable("incomplete_trace");
    }

    let mut sites = BTreeMap::new();
    let mut excluded = Excluded::default();
    for event in &trace.events {
        if cancel.load(Ordering::Relaxed) {
            return unavailable("cancelled");
        }
        let NesTraceWrite::Register { address, .. } = event.write else {
            excluded.non_register = excluded.non_register.saturating_add(1);
            continue;
        };
        if !is_apu_register(address) {
            excluded.non_apu_register = excluded.non_apu_register.saturating_add(1);
            continue;
        }
        let Some(pc) = u16::try_from(event.pc).ok().filter(|pc| *pc != 0) else {
            excluded.invalid_pc = excluded.invalid_pc.saturating_add(1);
            continue;
        };
        let AudioTraceSource::CartridgeRom {
            offset,
            bit_reversed,
        } = event.instruction_source
        else {
            excluded.non_cartridge_source = excluded.non_cartridge_source.saturating_add(1);
            continue;
        };
        if bit_reversed {
            excluded.bit_reversed_source = excluded.bit_reversed_source.saturating_add(1);
            continue;
        }
        let Some(instruction) = Instruction::read(source, nrom, pc, offset) else {
            excluded.invalid_instruction_source =
                excluded.invalid_instruction_source.saturating_add(1);
            continue;
        };
        let Some(addressing) = Addressing::parse(instruction.bytes, address) else {
            excluded.unsupported_or_unreachable_instruction = excluded
                .unsupported_or_unreachable_instruction
                .saturating_add(1);
            continue;
        };
        let key = SiteKey { pc, offset };
        if !sites.contains_key(&key) && sites.len() == MAX_SITES {
            return unavailable("site_limit");
        }
        sites
            .entry(key)
            .or_insert_with(|| Site::new(pc, offset, instruction, addressing))
            .record(address, event.cycle);
    }
    if cancel.load(Ordering::Relaxed) {
        return unavailable("cancelled");
    }
    let observed_event_count = sites.values().map(|site| site.observed.count).sum::<u64>();
    debug_assert_eq!(
        observed_event_count.saturating_add(excluded.total()),
        trace.events.len() as u64
    );

    json!({
        "schema": SCHEMA,
        "qualification": QUALIFICATION,
        "status": "complete",
        "identity": identity(evidence, row, trace),
        "limits": {"max_sites": MAX_SITES, "max_events": MAX_EVENTS},
        "event_count": trace.events.len(),
        "observed_event_count": observed_event_count,
        "excluded_events": excluded.json(),
        "sites": sites.into_values().map(Site::json).collect::<Vec<_>>(),
        "limitations": [
            "Sites record observed instruction-origin APU register writes only.",
            "Indexed stores show only that an observed register was reachable by an 8-bit index.",
            "Sites do not establish callers, selectors, functions, songs, durations, loops, or export items.",
        ],
    })
}

fn is_apu_register(address: u16) -> bool {
    matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
}

#[derive(Default)]
struct Excluded {
    non_register: u64,
    non_apu_register: u64,
    invalid_pc: u64,
    non_cartridge_source: u64,
    bit_reversed_source: u64,
    invalid_instruction_source: u64,
    unsupported_or_unreachable_instruction: u64,
}

impl Excluded {
    fn total(&self) -> u64 {
        self.non_register
            .saturating_add(self.non_apu_register)
            .saturating_add(self.invalid_pc)
            .saturating_add(self.non_cartridge_source)
            .saturating_add(self.bit_reversed_source)
            .saturating_add(self.invalid_instruction_source)
            .saturating_add(self.unsupported_or_unreachable_instruction)
    }

    fn json(&self) -> Value {
        json!({
            "non_register": self.non_register,
            "non_apu_register": self.non_apu_register,
            "invalid_pc": self.invalid_pc,
            "non_cartridge_source": self.non_cartridge_source,
            "bit_reversed_source": self.bit_reversed_source,
            "invalid_instruction_source": self.invalid_instruction_source,
            "unsupported_or_unreachable_instruction": self.unsupported_or_unreachable_instruction,
        })
    }
}

struct Instruction {
    bytes: [u8; 3],
    offsets: [u64; 3],
}

impl Instruction {
    fn read(source: &[u8], nrom: Nrom, pc: u16, first_offset: u64) -> Option<Self> {
        let mut bytes = [0; 3];
        let mut offsets = [0; 3];
        for delta in 0..3 {
            let address = pc.checked_add(delta)?;
            let offset = nrom.offset_for(address)?;
            if delta == 0 && offset != first_offset {
                return None;
            }
            bytes[usize::from(delta)] = *source.get(usize::try_from(offset).ok()?)?;
            offsets[usize::from(delta)] = offset;
        }
        Some(Self { bytes, offsets })
    }
}

#[derive(Clone, Copy)]
enum Addressing {
    Absolute { base: u16 },
    AbsoluteX { base: u16 },
    AbsoluteY { base: u16 },
}

impl Addressing {
    fn parse(bytes: [u8; 3], register: u16) -> Option<Self> {
        let base = u16::from_le_bytes([bytes[1], bytes[2]]);
        match bytes[0] {
            0x8c..=0x8e if base == register => Some(Self::Absolute { base }),
            0x9d if register.wrapping_sub(base) <= u16::from(u8::MAX) => {
                Some(Self::AbsoluteX { base })
            }
            0x99 if register.wrapping_sub(base) <= u16::from(u8::MAX) => {
                Some(Self::AbsoluteY { base })
            }
            _ => None,
        }
    }

    fn json(self) -> Value {
        let (mode, base_address) = match self {
            Self::Absolute { base } => ("absolute", base),
            Self::AbsoluteX { base } => ("absolute_x", base),
            Self::AbsoluteY { base } => ("absolute_y", base),
        };
        json!({"mode": mode, "base_address": base_address})
    }
}

#[derive(Ord, PartialOrd, Eq, PartialEq)]
struct SiteKey {
    pc: u16,
    offset: u64,
}

struct Site {
    pc: u16,
    offset: u64,
    instruction: Instruction,
    addressing: Addressing,
    registers: BTreeMap<u16, Observation>,
    observed: Observation,
}

impl Site {
    fn new(pc: u16, offset: u64, instruction: Instruction, addressing: Addressing) -> Self {
        Self {
            pc,
            offset,
            instruction,
            addressing,
            registers: BTreeMap::new(),
            observed: Observation::default(),
        }
    }

    fn record(&mut self, address: u16, cycle: u64) {
        self.registers.entry(address).or_default().record(cycle);
        self.observed.record(cycle);
    }

    fn json(self) -> Value {
        json!({
            "pc": self.pc,
            "instruction_source": {"kind": "cartridge_rom", "offset": self.offset, "bit_reversed": false},
            "instruction": {
                "bytes": const_hex::encode(self.instruction.bytes),
                "sha256": zeff_firmware::sha256_hex(&self.instruction.bytes),
                "source_offsets": self.instruction.offsets,
            },
            "addressing": self.addressing.json(),
            "registers": self.registers.into_iter().map(|(address, observed)| {
                json!({"address": address, "observed": observed.json()})
            }).collect::<Vec<_>>(),
            "observed": self.observed.json(),
        })
    }
}

#[derive(Default)]
struct Observation {
    count: u64,
    first_cycle: u64,
    last_cycle: u64,
}

impl Observation {
    fn record(&mut self, cycle: u64) {
        if self.count == 0 {
            self.first_cycle = cycle;
        }
        self.last_cycle = cycle;
        self.count = self.count.saturating_add(1);
    }

    fn json(&self) -> Value {
        json!({
            "count": self.count,
            "first_cycle": self.first_cycle,
            "last_cycle": self.last_cycle,
        })
    }
}

#[cfg(test)]
#[path = "runtime_writers/tests.rs"]
mod tests;
