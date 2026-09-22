use std::collections::BTreeMap;

use crate::{ScanStop, tracker::FileSpan};

use super::{Budget, MAX_RESIDENT_ROM, reachable_calls_to, span};

pub(super) struct DispatcherReport {
    pub(super) dispatchers: Vec<Dispatcher>,
    pub(super) invalid_tables: Vec<FileSpan>,
}

pub(super) struct Dispatcher {
    pub(super) at: usize,
    pub(super) count: u8,
    pub(super) table: usize,
    pub(super) call_sites: Vec<FileSpan>,
    pub(super) call_roots: Vec<u8>,
    pub(super) entries: Vec<Entry>,
}

pub(super) struct Entry {
    pub(super) selector: u8,
    pub(super) pointer: u32,
    pub(super) span: FileSpan,
}

pub(super) fn dispatchers(
    bytes: &[u8],
    reachable: &BTreeMap<usize, u8>,
    play: usize,
    budget: &mut Budget<'_>,
) -> Result<DispatcherReport, ScanStop> {
    let mut dispatchers = Vec::new();
    let mut invalid_tables = Vec::new();
    for &at in reachable.keys() {
        budget.charge(1)?;
        let Some(window) = bytes.get(at..at.saturating_add(18)) else {
            continue;
        };
        if window[0] != 0xfe
            || window[2..7] != [0xd0, 0x6f, 0x26, 0, 0x29]
            || window[7] != 0x11
            || window[10..16] != [0x19, 0x5e, 0x23, 0x56, 0xeb, 0xc3]
            || u16::from_le_bytes([window[16], window[17]]) as usize != play
        {
            continue;
        }
        let starts = [
            at,
            at + 2,
            at + 3,
            at + 4,
            at + 6,
            at + 7,
            at + 10,
            at + 11,
            at + 12,
            at + 13,
            at + 14,
            at + 15,
        ];
        let Some(roots) = starts
            .into_iter()
            .map(|start| reachable.get(&start).copied())
            .collect::<Option<Vec<_>>>()
            .map(|roots| {
                roots
                    .into_iter()
                    .fold(u8::MAX, |shared, roots| shared & roots)
            })
        else {
            continue;
        };
        if roots == 0 {
            continue;
        }
        let calls = reachable_calls_to(bytes, reachable, at, budget)?;
        let calls: Vec<_> = calls
            .into_iter()
            .filter(|(_, call_roots)| call_roots & roots != 0)
            .collect();
        if calls.is_empty() {
            continue;
        }
        let count = window[1];
        let table = u16::from_le_bytes([window[8], window[9]]) as usize;
        let Some(table_end) = table.checked_add(usize::from(count) * 2) else {
            invalid_tables.push(span(at, 18));
            continue;
        };
        if count == 0 || table_end > bytes.len() || table_end > MAX_RESIDENT_ROM {
            invalid_tables.push(span(at, 18));
            continue;
        }
        let mut entries = Vec::with_capacity(usize::from(count));
        for selector in 0..count {
            budget.charge(1)?;
            let entry = table + usize::from(selector) * 2;
            entries.push(Entry {
                selector,
                pointer: u16::from_le_bytes([bytes[entry], bytes[entry + 1]]) as u32,
                span: span(entry, 2),
            });
        }
        dispatchers.push(Dispatcher {
            at,
            count,
            table,
            call_sites: calls.iter().map(|(site, _)| *site).collect(),
            call_roots: calls.iter().map(|(_, roots)| *roots).collect(),
            entries,
        });
    }
    Ok(DispatcherReport {
        dispatchers,
        invalid_tables,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn last_guarded_entry_uses_a_wide_index_and_requires_every_instruction_boundary() {
        let mut bytes = vec![0xc9; 0x8000];
        bytes[..3].copy_from_slice(&[0xcd, 0, 1]);
        bytes[0x100..0x112].copy_from_slice(&[
            0xfe, 255, 0xd0, 0x6f, 0x26, 0, 0x29, 0x11, 0, 4, 0x19, 0x5e, 0x23, 0x56, 0xeb, 0xc3,
            0, 2,
        ]);
        bytes[0x5fc..0x5fe].copy_from_slice(&[0x34, 0x12]);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(crate::ScanLimits::default(), &cancel).unwrap();
        let reachable =
            super::super::control_flow::reachable_instructions(&bytes, &mut budget).unwrap();
        let report = dispatchers(&bytes, &reachable, 0x200, &mut budget).unwrap();
        assert!(report.invalid_tables.is_empty());
        assert_eq!(report.dispatchers.len(), 1);
        let entries = &report.dispatchers[0].entries;
        assert_eq!(entries.len(), 255);
        assert_eq!(
            (
                entries[254].selector,
                entries[254].span.offset,
                entries[254].pointer
            ),
            (254, 0x5fc, 0x1234)
        );
        let mut missing_boundary = reachable;
        missing_boundary.remove(&0x10d);
        assert!(
            dispatchers(&bytes, &missing_boundary, 0x200, &mut budget)
                .unwrap()
                .dispatchers
                .is_empty()
        );
    }
}
