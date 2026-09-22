use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::{SOURCE_REVISION, SongStructure, inspect_with_budget, span, word};
use crate::{ScanLimits, ScanStop, tracker::FileSpan};

mod control_flow;
pub(in crate::huge) mod reference;

#[cfg(any(test, feature = "test-support"))]
pub(super) fn fixture_driver(bytes: &mut [u8], start: u16, ram: u16) -> u16 {
    for (index, slot) in bytes[usize::from(start)..][..reference::CODE.len()]
        .iter_mut()
        .enumerate()
    {
        *slot = reference::relocated_byte(index, start, ram);
    }
    start + reference::UPDATE_OFFSET as u16
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryReport {
    pub bound: Vec<BoundSong>,
    pub held: Vec<HeldEvidence>,
    pub work_used: u64,
    pub candidate_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BoundSong {
    pub song: SongStructure,
    pub init_calls: Vec<CallSite>,
    pub evidence: ExecutableEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CallSite {
    pub instruction: FileSpan,
    pub roots: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExecutableEvidence {
    pub source_revision: &'static str,
    pub driver: FileSpan,
    pub init_address: u16,
    pub update_address: u16,
    pub ram_address: u16,
    pub irq_update_calls: Vec<CallSite>,
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
    NoIrqUpdateCall,
    NoLiteralInitCall,
    InvalidDescriptorPointer,
    UnsupportedSong,
    DriverDataOverlap,
}

pub fn discover(
    bytes: &[u8],
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<DiscoveryReport, ScanStop> {
    if limits.max_work > crate::MAX_SCAN_WORK || limits.max_candidates > crate::MAX_CANDIDATES {
        return Err(ScanStop::InvalidLimits);
    }
    let mut inner = crate::Budget {
        cancel,
        remaining: limits.max_work,
    };
    discover_with_budget(bytes, &mut inner, limits.max_candidates as usize)
}

pub(crate) fn discover_with_budget(
    bytes: &[u8],
    inner: &mut crate::Budget<'_>,
    capacity: usize,
) -> Result<DiscoveryReport, ScanStop> {
    let mut budget = Budget::new(inner, capacity)?;
    budget.charge(0)?;
    let mut bound = Vec::new();
    let mut held = Vec::new();
    if bytes.len() != 0x8000 || bytes[0x147..0x14a] != [0, 0, 0] {
        held.push(HeldEvidence {
            kind: HeldKind::UnsupportedMapping,
            span: span(0, bytes.len().min(u32::MAX as usize)),
        });
        return Ok(budget.report(bound, held));
    }
    let drivers = find_drivers(bytes, &mut budget)?;
    if drivers.is_empty() {
        return Ok(budget.report(bound, held));
    }
    let reachable = control_flow::reachable_instructions(bytes, &mut budget)?;
    for (start, ram) in drivers {
        let driver_span = span(start, reference::CODE.len());
        let update = start + reference::UPDATE_OFFSET;
        let mut irq_update_calls = Vec::new();
        let mut descriptors = BTreeMap::<u16, Vec<CallSite>>::new();
        for (&at, &roots) in &reachable {
            budget.charge(1)?;
            if is_call_to(bytes, at, update) && roots & (2 | 8) != 0 {
                irq_update_calls.push(CallSite {
                    instruction: span(at, 3),
                    roots,
                });
            }
            if roots & 1 != 0
                && bytes[at] == 0x21
                && bytes.get(at..at + 6).is_some()
                && bytes[at + 3] == 0xcd
                && word(bytes, at + 4) == start
            {
                budget.candidate()?;
                descriptors
                    .entry(word(bytes, at + 1) as u16)
                    .or_default()
                    .push(CallSite {
                        instruction: span(at + 3, 3),
                        roots,
                    });
            }
        }
        if irq_update_calls.is_empty() {
            held.push(HeldEvidence {
                kind: HeldKind::NoIrqUpdateCall,
                span: driver_span,
            });
            continue;
        }
        if descriptors.is_empty() {
            held.push(HeldEvidence {
                kind: HeldKind::NoLiteralInitCall,
                span: driver_span,
            });
            continue;
        }
        for (descriptor, init_calls) in descriptors {
            if bytes
                .get(usize::from(descriptor)..usize::from(descriptor) + 21)
                .is_none()
            {
                held.push(HeldEvidence {
                    kind: HeldKind::InvalidDescriptorPointer,
                    span: init_calls[0].instruction,
                });
                continue;
            }
            let Some(song) = inspect_with_budget(bytes, descriptor, budget.inner)? else {
                held.push(HeldEvidence {
                    kind: HeldKind::UnsupportedSong,
                    span: span(descriptor.into(), 21),
                });
                continue;
            };
            if song.spans.iter().any(|s| overlaps(*s, driver_span)) {
                held.push(HeldEvidence {
                    kind: HeldKind::DriverDataOverlap,
                    span: song.descriptor,
                });
                continue;
            }
            bound.push(BoundSong {
                song,
                init_calls,
                evidence: ExecutableEvidence {
                    source_revision: SOURCE_REVISION,
                    driver: driver_span,
                    init_address: start as u16,
                    update_address: update as u16,
                    ram_address: ram,
                    irq_update_calls: irq_update_calls.clone(),
                },
            });
        }
    }
    budget.charge(0)?;
    Ok(budget.report(bound, held))
}

fn overlaps(a: FileSpan, b: FileSpan) -> bool {
    a.offset < b.offset + b.byte_len && b.offset < a.offset + a.byte_len
}

fn is_call_to(bytes: &[u8], at: usize, target: usize) -> bool {
    bytes.get(at..at + 3).is_some()
        && matches!(bytes[at], 0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc)
        && word(bytes, at + 1) == target
}

fn find_drivers(bytes: &[u8], budget: &mut Budget<'_, '_>) -> Result<Vec<(usize, u16)>, ScanStop> {
    let mut drivers = Vec::new();
    for start in 0..=0x4000 - reference::CODE.len() {
        budget.charge(1)?;
        let candidate = &bytes[start..start + reference::CODE.len()];
        if candidate[0] != reference::CODE[0] {
            continue;
        }
        let Some(ram) = reference::infer_ram(candidate) else {
            continue;
        };
        if usize::from(ram) < 0xc000 || usize::from(ram) + reference::RAM_SIZE > 0xd000 {
            continue;
        }
        let mut matched = true;
        for (index, &actual) in candidate.iter().enumerate() {
            budget.charge(1)?;
            if actual != reference::relocated_byte(index, start as u16, ram) {
                matched = false;
                break;
            }
        }
        if matched {
            budget.candidate()?;
            drivers.push((start, ram));
        }
    }
    Ok(drivers)
}

struct Budget<'a, 'b> {
    inner: &'a mut crate::Budget<'b>,
    start: u64,
    capacity: u32,
    candidates: u32,
}

impl<'a, 'b> Budget<'a, 'b> {
    fn new(inner: &'a mut crate::Budget<'b>, capacity: usize) -> Result<Self, ScanStop> {
        let capacity = u32::try_from(capacity).map_err(|_| ScanStop::CandidateLimit)?;
        Ok(Self {
            start: inner.remaining,
            inner,
            capacity,
            candidates: 0,
        })
    }

    fn charge(&mut self, amount: u64) -> Result<(), ScanStop> {
        if amount == 0 && self.inner.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(ScanStop::Cancelled);
        }
        for _ in 0..amount {
            self.inner.charge()?;
        }
        Ok(())
    }

    fn candidate(&mut self) -> Result<(), ScanStop> {
        if self.candidates >= self.capacity {
            return Err(ScanStop::CandidateLimit);
        }
        self.candidates += 1;
        Ok(())
    }

    fn report(self, bound: Vec<BoundSong>, held: Vec<HeldEvidence>) -> DiscoveryReport {
        DiscoveryReport {
            bound,
            held,
            work_used: self.start - self.inner.remaining,
            candidate_count: self.candidates,
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
