use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use super::{SOURCE_REVISION, decode};
use crate::{ScanLimits, ScanStop, tracker::FileSpan};

const VARIANT: &str = "devkitsms-psglib-f433a35d-sdcc-4.5.0";
const MAX_RESIDENT_ROM: usize = 0x8000;

mod control_flow;
pub(super) mod fingerprint;
mod tables;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryReport {
    pub bound: Vec<BoundSequence>,
    pub held: Vec<HeldEvidence>,
    pub work_used: u64,
    pub candidate_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BoundSequence {
    pub offset: u32,
    pub call_sites: Vec<FileSpan>,
    pub call_roots: Vec<u8>,
    pub frames: u32,
    pub write_count: u32,
    pub spans: Vec<FileSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_offset: Option<u32>,
    pub end_offset: u32,
    pub evidence: ExecutableEvidence,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub table_entries: Vec<TableEntryEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableEntryEvidence {
    pub selector: u8,
    pub count: u8,
    pub dispatcher: FileSpan,
    pub table: FileSpan,
    pub entry: FileSpan,
    pub call_sites: Vec<FileSpan>,
    pub call_roots: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExecutableEvidence {
    pub variant: &'static str,
    pub source_revision: &'static str,
    pub psg_play: FileSpan,
    pub psg_frame: FileSpan,
    pub code_delta: i32,
    pub ram_delta: i32,
    pub frame_call_sites: Vec<FileSpan>,
    pub frame_call_roots: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psg_play_loops: Option<FileSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HeldEvidence {
    pub kind: HeldKind,
    pub span: FileSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldKind {
    UnsupportedMapping,
    FingerprintOnly,
    IndirectOrUnreachableCall,
    InvalidPointer,
    InvalidTable,
    InvalidStream,
}

pub fn discover(
    bytes: &[u8],
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<DiscoveryReport, ScanStop> {
    let mut budget = Budget::new(limits, cancel)?;
    budget.charge(0)?;
    if bytes.len() > MAX_RESIDENT_ROM {
        return Ok(DiscoveryReport {
            bound: Vec::new(),
            held: vec![HeldEvidence {
                kind: HeldKind::UnsupportedMapping,
                span: span(0, bytes.len().min(u32::MAX as usize)),
            }],
            work_used: 0,
            candidate_count: 0,
        });
    }
    let reachable = control_flow::reachable_instructions(bytes, &mut budget)?;
    let mut drivers = find_drivers(bytes, &mut budget)?;
    if drivers.is_empty() {
        return Ok(budget.report(Vec::new(), Vec::new()));
    }
    let mut held: Vec<_> = drivers
        .iter()
        .map(|driver| HeldEvidence {
            kind: HeldKind::FingerprintOnly,
            span: span(driver.frame, 0x177),
        })
        .collect();
    let mut streams = BTreeMap::<(u32, usize, Option<usize>), BoundSequence>::new();
    let mut bound_frames = BTreeSet::new();
    for driver in &mut drivers {
        let frame_calls = reachable_calls_to(bytes, &reachable, driver.frame, &mut budget)?;
        driver.frame_call_sites = frame_calls.iter().map(|(site, _)| *site).collect();
        driver.frame_call_roots = frame_calls.iter().map(|(_, roots)| *roots).collect();
        if driver.frame_call_sites.is_empty() {
            continue;
        }
        for site in call_sites(bytes, &reachable, driver, &mut budget)? {
            budget.candidate()?;
            let pointer = site.pointer as usize;
            if pointer >= bytes.len() {
                held.push(HeldEvidence {
                    kind: HeldKind::InvalidPointer,
                    span: site.span,
                });
                continue;
            }
            // Reserving the decoder's fixed maxima means a budget failure cannot leak a partial set.
            budget.reserve_decode()?;
            let decoded = match decode(bytes, site.pointer, cancel) {
                Ok(decoded) => decoded,
                Err(_) if cancel.load(Ordering::Relaxed) => return Err(ScanStop::Cancelled),
                Err(_) => {
                    held.push(HeldEvidence {
                        kind: HeldKind::InvalidStream,
                        span: site.span,
                    });
                    continue;
                }
            };
            let key = (site.pointer, driver.play, site.loops);
            let entry = streams.entry(key).or_insert_with(|| BoundSequence {
                offset: site.pointer,
                call_sites: Vec::new(),
                call_roots: Vec::new(),
                frames: decoded.frames,
                write_count: decoded.writes.len() as u32,
                spans: decoded.spans,
                loop_offset: decoded.loop_offset,
                end_offset: decoded.end_offset,
                evidence: driver.evidence(site.loops),
                table_entries: Vec::new(),
            });
            entry.call_sites.push(site.span);
            entry.call_roots.push(site.roots);
            bound_frames.insert(driver.frame as u32);
        }
        let table_report = tables::dispatchers(bytes, &reachable, driver.play, &mut budget)?;
        held.extend(
            table_report
                .invalid_tables
                .into_iter()
                .map(|span| HeldEvidence {
                    kind: HeldKind::InvalidTable,
                    span,
                }),
        );
        for dispatcher in table_report.dispatchers {
            for entry in dispatcher.entries {
                budget.candidate()?;
                let pointer = entry.pointer as usize;
                if pointer >= bytes.len() {
                    held.push(HeldEvidence {
                        kind: HeldKind::InvalidPointer,
                        span: entry.span,
                    });
                    continue;
                }
                budget.reserve_decode()?;
                let decoded = match decode(bytes, entry.pointer, cancel) {
                    Ok(decoded) => decoded,
                    Err(_) if cancel.load(Ordering::Relaxed) => return Err(ScanStop::Cancelled),
                    Err(_) => {
                        held.push(HeldEvidence {
                            kind: HeldKind::InvalidStream,
                            span: entry.span,
                        });
                        continue;
                    }
                };
                let key = (entry.pointer, driver.play, None);
                let sequence = streams.entry(key).or_insert_with(|| BoundSequence {
                    offset: entry.pointer,
                    call_sites: Vec::new(),
                    call_roots: Vec::new(),
                    frames: decoded.frames,
                    write_count: decoded.writes.len() as u32,
                    spans: decoded.spans,
                    loop_offset: decoded.loop_offset,
                    end_offset: decoded.end_offset,
                    evidence: driver.evidence(None),
                    table_entries: Vec::new(),
                });
                sequence.table_entries.push(TableEntryEvidence {
                    selector: entry.selector,
                    count: dispatcher.count,
                    dispatcher: span(dispatcher.at, 18),
                    table: span(dispatcher.table, dispatcher.count as usize * 2),
                    entry: entry.span,
                    call_sites: dispatcher.call_sites.clone(),
                    call_roots: dispatcher.call_roots.clone(),
                });
                bound_frames.insert(driver.frame as u32);
            }
        }
    }
    for sequence in streams.values_mut() {
        let mut calls: Vec<_> = sequence
            .call_sites
            .iter()
            .copied()
            .zip(sequence.call_roots.iter().copied())
            .collect();
        calls.sort_by_key(|(site, _)| site.offset);
        sequence.call_sites = calls.iter().map(|(site, _)| *site).collect();
        sequence.call_roots = calls.iter().map(|(_, roots)| *roots).collect();
        sequence
            .table_entries
            .sort_by_key(|entry| (entry.dispatcher.offset, entry.selector, entry.entry.offset));
    }
    held.retain(|evidence| {
        evidence.kind != HeldKind::FingerprintOnly || !bound_frames.contains(&evidence.span.offset)
    });
    budget.charge(0)?;
    held.sort_by_key(|evidence| (evidence.span.offset, evidence.kind as u8));
    Ok(budget.report(streams.into_values().collect(), held))
}

pub(super) struct Budget<'a> {
    limits: ScanLimits,
    cancel: &'a AtomicBool,
    used: u64,
    candidates: u32,
}

impl<'a> Budget<'a> {
    pub(super) fn new(limits: ScanLimits, cancel: &'a AtomicBool) -> Result<Self, ScanStop> {
        if limits.max_work > crate::MAX_SCAN_WORK || limits.max_candidates > crate::MAX_CANDIDATES {
            return Err(ScanStop::InvalidLimits);
        }
        Ok(Self {
            limits,
            cancel,
            used: 0,
            candidates: 0,
        })
    }

    pub(super) fn charge(&mut self, amount: u64) -> Result<(), ScanStop> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ScanStop::Cancelled);
        }
        self.used = self.used.checked_add(amount).ok_or(ScanStop::WorkLimit)?;
        if self.used > self.limits.max_work {
            return Err(ScanStop::WorkLimit);
        }
        Ok(())
    }

    pub(super) fn candidate(&mut self) -> Result<(), ScanStop> {
        self.candidates = self
            .candidates
            .checked_add(1)
            .ok_or(ScanStop::CandidateLimit)?;
        if self.candidates > self.limits.max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        self.charge(1)
    }

    pub(super) fn reserve_decode(&mut self) -> Result<(), ScanStop> {
        self.charge((1_000_000 + super::MAX_STREAM_BYTES + super::MAX_WRITES) as u64)
    }

    fn report(&self, bound: Vec<BoundSequence>, held: Vec<HeldEvidence>) -> DiscoveryReport {
        DiscoveryReport {
            bound,
            held,
            work_used: self.used,
            candidate_count: self.candidates,
        }
    }
}

#[derive(Clone)]
struct Driver {
    play: usize,
    frame: usize,
    loops: Vec<usize>,
    ram_base: u16,
    frame_call_sites: Vec<FileSpan>,
    frame_call_roots: Vec<u8>,
}

impl Driver {
    fn evidence(&self, loops: Option<usize>) -> ExecutableEvidence {
        ExecutableEvidence {
            variant: VARIANT,
            source_revision: SOURCE_REVISION,
            psg_play: span(self.play, 57),
            psg_frame: span(self.frame, 0x177),
            code_delta: self.play as i32 - 0x11d,
            ram_delta: self.ram_base as i32 - 0xc000,
            frame_call_sites: self.frame_call_sites.clone(),
            frame_call_roots: self.frame_call_roots.clone(),
            psg_play_loops: loops.map(|at| span(at, 22)),
        }
    }
}

fn find_drivers(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    let mut drivers = Vec::new();
    for start in 0..bytes.len() {
        budget.charge(1)?;
        let Some(data) = bytes.get(start..start + fingerprint::REFERENCE.len()) else {
            break;
        };
        if data[0] != fingerprint::REFERENCE[0] || data[3..5] != fingerprint::REFERENCE[3..5] {
            continue;
        }
        let base = u16::from_le_bytes([data[1], data[2]]);
        if !(0xc000..=0xdfce).contains(&base) {
            continue;
        }
        if !matches_driver(bytes, start, base, budget)? {
            continue;
        }
        let play = start + fingerprint::PSG_PLAY_RELATIVE;
        let loops = find_loops(bytes, play, base, budget)?;
        drivers.push(Driver {
            play,
            frame: start + fingerprint::PSG_FRAME_RELATIVE,
            loops,
            ram_base: base,
            frame_call_sites: Vec::new(),
            frame_call_roots: Vec::new(),
        });
    }
    Ok(drivers)
}

pub(super) fn matches_driver(
    bytes: &[u8],
    start: usize,
    base: u16,
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    let Some(data) = bytes.get(start..start + fingerprint::REFERENCE.len()) else {
        return Ok(false);
    };
    let code_delta = start as i32 - fingerprint::REFERENCE_START as i32;
    let ram_delta = i32::from(base) - 0xc000;
    let mut at = 0;
    while at < data.len() {
        budget.charge(1)?;
        let code = fingerprint::CODE_OPERANDS.binary_search(&at).is_ok();
        let ram = fingerprint::RAM_OPERANDS.binary_search(&at).is_ok();
        if code || ram {
            let reference =
                u16::from_le_bytes([fingerprint::REFERENCE[at], fingerprint::REFERENCE[at + 1]]);
            let expected = i32::from(reference) + if code { code_delta } else { ram_delta };
            let actual = i32::from(u16::from_le_bytes([data[at], data[at + 1]]));
            if expected != actual || (code && !(0..bytes.len() as i32).contains(&expected)) {
                return Ok(false);
            }
            at += 2;
        } else {
            if data[at] != fingerprint::REFERENCE[at] {
                return Ok(false);
            }
            at += 1;
        }
    }
    Ok(true)
}

pub(super) fn reachable_calls_to(
    bytes: &[u8],
    reachable: &BTreeMap<usize, u8>,
    target: usize,
    budget: &mut Budget<'_>,
) -> Result<Vec<(FileSpan, u8)>, ScanStop> {
    let mut sites = Vec::new();
    for &at in reachable.keys() {
        budget.charge(1)?;
        if bytes.get(at) == Some(&0xcd)
            && bytes.get(at + 1..at + 3).is_some_and(|operand| {
                u16::from_le_bytes([operand[0], operand[1]]) as usize == target
            })
        {
            sites.push((span(at, 3), reachable[&at]));
        }
    }
    Ok(sites)
}

fn find_loops(
    bytes: &[u8],
    play: usize,
    base: u16,
    budget: &mut Budget<'_>,
) -> Result<Vec<usize>, ScanStop> {
    let mut loops = Vec::new();
    for at in 0..bytes.len() {
        budget.charge(1)?;
        let Some(data) = bytes.get(at..at.saturating_add(22)) else {
            continue;
        };
        let word = |index| u16::from_le_bytes([data[index], data[index + 1]]);
        if data[0] == 0xcd
            && word(1) == play as u16
            && data[3..5] == [0xaf, 0x32]
            && word(5) == base + 8
            && data[7..14] == [0xfd, 0x21, 2, 0, 0xfd, 0x39, 0xfd]
            && data[14..16] == [0x7e, 0]
            && data[16] == 0x32
            && word(17) == base + 9
            && data[19..22] == [0xe1, 0x33, 0xe9]
        {
            loops.push(at);
        }
    }
    Ok(loops)
}

#[derive(Clone, Copy)]
struct CallSite {
    pointer: u32,
    span: FileSpan,
    roots: u8,
    loops: Option<usize>,
}

fn call_sites(
    bytes: &[u8],
    reachable: &BTreeMap<usize, u8>,
    driver: &Driver,
    budget: &mut Budget<'_>,
) -> Result<Vec<CallSite>, ScanStop> {
    let mut sites = Vec::new();
    for &at in reachable.keys() {
        budget.charge(1)?;
        let Some(window) = bytes.get(at..at.saturating_add(6)) else {
            continue;
        };
        if window[0] != 0x21 || window[3] != 0xcd {
            continue;
        }
        let roots = reachable[&at] & reachable.get(&(at + 3)).copied().unwrap_or_default();
        if roots == 0 {
            continue;
        }
        let target = u16::from_le_bytes([window[4], window[5]]) as usize;
        let loops = if target == driver.play {
            None
        } else if driver.loops.binary_search(&target).is_ok() {
            Some(target)
        } else {
            continue;
        };
        sites.push(CallSite {
            pointer: u16::from_le_bytes([window[1], window[2]]) as u32,
            span: span(at, 6),
            roots,
            loops,
        });
    }
    Ok(sites)
}

pub(super) fn span(offset: usize, byte_len: usize) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len: byte_len as u32,
    }
}
