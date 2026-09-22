use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail, ensure};
use serde::Serialize;

use super::{VgmLog, decode, verify};

const SEGA_CLOCKS: &[u32] = &[3_579_545, 3_546_893, 3_584_160, 3_568_200];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnPsgModel {
    Sega,
    TiSn76489,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SnPlayback {
    pub clock_hz: u32,
    pub model: SnPsgModel,
    pub stereo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnWrite {
    Psg(u8),
    Stereo(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedSnWrite {
    pub tick: u64,
    pub write: SnWrite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSnVgm {
    pub config: SnPlayback,
    pub writes: Vec<TimedSnWrite>,
    pub duration_ticks: u64,
}

pub(super) fn capability(data: &[u8], log: &VgmLog) -> Option<SnPlayback> {
    if log.version < 0x161
        || log.samples == 0
        || !log.warnings.is_empty()
        || log.chips.len() != 1
        || log.chips[0].name != "sn76489"
    {
        return None;
    }
    let clock = log.chips[0].raw_clock;
    if clock & 0xc000_0000 != 0 || header_byte(data, log, 0x7c).is_some_and(|value| value != 0) {
        return None;
    }
    let feedback =
        u16::from_le_bytes([header_byte(data, log, 0x28)?, header_byte(data, log, 0x29)?]);
    let width = header_byte(data, log, 0x2a)?;
    let flags = header_byte(data, log, 0x2b)?;
    let model =
        if feedback == 9 && width == 16 && matches!(flags, 0 | 4) && SEGA_CLOCKS.contains(&clock) {
            SnPsgModel::Sega
        } else if feedback == 3 && width == 15 && flags == 5 && clock == 3_579_545 {
            SnPsgModel::TiSn76489
        } else {
            return None;
        };
    let mut psg_writes = 0u32;
    let stereo = flags == 0;
    for (&opcode, &count) in &log.command_histogram {
        if count == 0 {
            continue;
        }
        match opcode {
            0x4f if !stereo => return None,
            0x4f => {}
            0x50 => psg_writes = psg_writes.saturating_add(count),
            0x61..=0x63 | 0x66 | 0x70..=0x7f => {}
            _ => return None,
        }
    }
    (psg_writes > 0).then_some(SnPlayback {
        clock_hz: clock,
        model,
        stereo,
    })
}

fn header_byte(data: &[u8], log: &VgmLog, offset: usize) -> Option<u8> {
    (log.header.offset == 0 && offset < log.header.byte_len as usize)
        .then(|| data.get(offset).copied())
        .flatten()
}

pub fn prepare(bytes: &[u8], log: &VgmLog, cancel: &AtomicBool) -> Result<PreparedSnVgm> {
    ensure!(
        log.sn_playback.is_some(),
        "VGM does not have a qualified standalone SN76489 playback contract"
    );
    verify(bytes, log, cancel)?;
    let data = decode(bytes, cancel)?;
    if cancel.load(Ordering::Relaxed) {
        bail!("VGM preparation cancelled");
    }
    let config = capability(&data, log)
        .ok_or_else(|| anyhow::anyhow!("VGM playback capability no longer matches its source"))?;
    ensure!(
        log.sn_playback == Some(config),
        "VGM playback configuration no longer matches its source"
    );
    let start = log.commands.offset as usize;
    let end = start
        .checked_add(log.commands.byte_len as usize)
        .ok_or_else(|| anyhow::anyhow!("VGM command span overflows"))?;
    let commands = data
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("VGM command span is out of bounds"))?;
    let mut writes = Vec::new();
    let mut tick = 0u64;
    let mut pos = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("VGM preparation cancelled");
        }
        let opcode = *commands
            .get(pos)
            .ok_or_else(|| anyhow::anyhow!("VGM command stream is truncated"))?;
        match opcode {
            0x4f => push_write(
                &mut writes,
                tick,
                SnWrite::Stereo(command_byte(commands, pos)?),
            ),
            0x50 => push_write(
                &mut writes,
                tick,
                SnWrite::Psg(command_byte(commands, pos)?),
            ),
            0x61 => {
                tick = tick
                    .checked_add(u64::from(command_u16(commands, pos)?))
                    .ok_or_else(|| anyhow::anyhow!("VGM duration overflows"))?
            }
            0x62 => {
                tick = tick
                    .checked_add(735)
                    .ok_or_else(|| anyhow::anyhow!("VGM duration overflows"))?
            }
            0x63 => {
                tick = tick
                    .checked_add(882)
                    .ok_or_else(|| anyhow::anyhow!("VGM duration overflows"))?
            }
            0x70..=0x7f => {
                tick = tick
                    .checked_add(u64::from((opcode & 15) + 1))
                    .ok_or_else(|| anyhow::anyhow!("VGM duration overflows"))?
            }
            0x66 => {
                ensure!(
                    pos + 1 == commands.len(),
                    "VGM command span has trailing data"
                );
                break;
            }
            _ => bail!("VGM command {opcode:#04x} is unsupported for SN76489 playback"),
        }
        pos = pos
            .checked_add(command_len(opcode)?)
            .ok_or_else(|| anyhow::anyhow!("VGM command position overflows"))?;
    }
    ensure!(
        tick == log.samples,
        "VGM duration no longer matches its inventory"
    );
    ensure!(
        tick > 0 && !writes.is_empty(),
        "VGM has no playable duration or PSG writes"
    );
    Ok(PreparedSnVgm {
        config,
        writes,
        duration_ticks: tick,
    })
}

fn push_write(writes: &mut Vec<TimedSnWrite>, tick: u64, write: SnWrite) {
    writes.push(TimedSnWrite { tick, write });
}

fn command_len(opcode: u8) -> Result<usize> {
    match opcode {
        0x4f | 0x50 => Ok(2),
        0x61 => Ok(3),
        0x62 | 0x63 | 0x66 | 0x70..=0x7f => Ok(1),
        _ => bail!("unsupported VGM command length"),
    }
}

fn command_byte(commands: &[u8], pos: usize) -> Result<u8> {
    commands
        .get(pos + 1)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("VGM write is truncated"))
}

fn command_u16(commands: &[u8], pos: usize) -> Result<u16> {
    let bytes = commands
        .get(pos + 1..pos + 3)
        .ok_or_else(|| anyhow::anyhow!("VGM wait is truncated"))?;
    Ok(u16::from_le_bytes(
        bytes.try_into().expect("two-byte command slice"),
    ))
}

#[cfg(test)]
mod tests;
