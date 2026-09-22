use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;

use super::discovery::{BoundSong, discover};
use super::span;
use crate::tracker::FileSpan;

#[derive(Debug, Serialize)]
pub struct IsolatedRom {
    #[serde(skip)]
    pub bytes: Vec<u8>,
    pub copied_spans: Vec<FileSpan>,
    pub bootstrap_spans: Vec<FileSpan>,
    pub descriptor: u16,
    pub driver_start: u16,
    pub driver_end: u16,
    pub ram_start: u16,
    pub ram_end: u16,
    pub init_call: u16,
    pub update_call: u16,
}

pub fn build(
    bytes: &[u8],
    selected: &BoundSong,
    fill: u8,
    cancel: &AtomicBool,
) -> Result<IsolatedRom> {
    let report = discover(bytes, Default::default(), cancel)
        .map_err(|stop| anyhow::anyhow!("hUGE isolation validation stopped: {stop:?}"))?;
    ensure!(
        report.bound.contains(selected),
        "hUGE selection no longer matches its source"
    );
    build_from_bound(bytes, selected, fill, cancel)
}

pub(in crate::huge) fn bootstrap_matches(
    bytes: &[u8],
    selected: &BoundSong,
    budget: &mut crate::Budget<'_>,
) -> std::result::Result<bool, crate::ScanStop> {
    let isolated = match build_from_bound(bytes, selected, 0, budget.cancel) {
        Ok(isolated) => isolated,
        Err(_) if budget.cancel.load(Ordering::Relaxed) => return Err(crate::ScanStop::Cancelled),
        Err(_) => return Ok(false),
    };
    for span in &isolated.bootstrap_spans {
        let start = span.offset as usize;
        let end = start + span.byte_len as usize;
        for (actual, expected) in bytes[start..end].iter().zip(&isolated.bytes[start..end]) {
            budget.charge()?;
            if actual != expected {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(in crate::huge) fn build_from_bound(
    bytes: &[u8],
    selected: &BoundSong,
    fill: u8,
    cancel: &AtomicBool,
) -> Result<IsolatedRom> {
    ensure!(
        bytes[0x143] == 0,
        "isolation currently requires a DMG cartridge"
    );
    let mut copied_spans = selected.song.spans.clone();
    copied_spans.push(selected.evidence.driver);
    copied_spans.sort_by_key(|s| s.offset);
    ensure!(
        copied_spans.iter().all(|s| s.offset >= 0x200),
        "audio data overlaps the isolation bootstrap"
    );
    let descriptor = u16::try_from(selected.song.descriptor.offset)?;
    let init = selected.evidence.init_address;
    let update = selected.evidence.update_address;
    let mut result = vec![fill; 0x8000];
    for source in &copied_spans {
        let start = source.offset as usize;
        let end = start + source.byte_len as usize;
        result[start..end].copy_from_slice(&bytes[start..end]);
    }
    result[..0x200].fill(0);
    result[0x100..0x104].copy_from_slice(&[0, 0xc3, 0x50, 1]);
    result[0x104..0x134].copy_from_slice(&bytes[0x104..0x134]);
    result[0x134..0x140].copy_from_slice(b"HUGE ISOLATE");
    result[0x14b] = 0x33;
    let mut player = vec![
        0xf3, 0x31, 0xfe, 0xff, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x26, 0xe0, 0x81, 0x3e, 0x80,
        0xe0, 0x26, 0x3e, 0xff, 0xe0, 0x25, 0x3e, 0x77, 0xe0, 0x24, 0x21,
    ];
    player.extend_from_slice(&descriptor.to_le_bytes());
    let init_call = 0x150 + player.len() as u16;
    player.push(0xcd);
    player.extend_from_slice(&init.to_le_bytes());
    player.extend_from_slice(&[
        0xaf, 0xe0, 0x0f, 0x3c, 0xe0, 0x80, 0xe0, 0xff, 0xfb, 0x76, 0, 0x18, 0xfc,
    ]);
    let irq = 0x150 + player.len() as u16;
    player.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5]);
    let update_call = 0x150 + player.len() as u16;
    player.push(0xcd);
    player.extend_from_slice(&update.to_le_bytes());
    player.extend_from_slice(&[0xf0, 0x81, 0x3c, 0xe0, 0x81, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    result[0x40..0x43].copy_from_slice(&[0xc3, irq as u8, (irq >> 8) as u8]);
    result[0x150..0x150 + player.len()].copy_from_slice(&player);
    result[0x14d] = result[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_sub(*b).wrapping_sub(1));
    let checksum = result
        .iter()
        .enumerate()
        .filter(|(i, _)| !matches!(i, 0x14e | 0x14f))
        .fold(0u16, |sum, (_, b)| sum.wrapping_add(u16::from(*b)));
    result[0x14e..0x150].copy_from_slice(&checksum.to_be_bytes());
    ensure!(!cancel.load(Ordering::Relaxed), "hUGE isolation cancelled");
    Ok(IsolatedRom {
        bytes: result,
        copied_spans,
        bootstrap_spans: vec![span(0x40, 3), span(0x100, 4), span(0x150, player.len())],
        descriptor,
        driver_start: init,
        driver_end: u16::try_from(
            selected.evidence.driver.offset + selected.evidence.driver.byte_len,
        )?,
        ram_start: selected.evidence.ram_address,
        ram_end: selected.evidence.ram_address + 100,
        init_call,
        update_call,
    })
}

#[cfg(test)]
mod tests;
