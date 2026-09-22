use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail, ensure};

use super::{DecodedStream, FrameWrite, MAX_FRAMES, MAX_STREAM_BYTES, MAX_WRITES};
use crate::tracker::FileSpan;

pub(super) fn decode(bytes: &[u8], offset: u32, cancel: &AtomicBool) -> Result<DecodedStream> {
    ensure!(
        bytes.len() <= crate::MAX_ROM_BYTES,
        "PSGlib source exceeds media limit"
    );
    let start = offset as usize;
    ensure!(start < bytes.len(), "PSGlib stream starts outside source");
    let end = bytes.len().min(start + MAX_STREAM_BYTES);
    let mut state = Decoder {
        bytes,
        start,
        end,
        cursor: start,
        substring: None,
        visited: BTreeSet::new(),
        writes: Vec::new(),
        frame: 0,
        last_latch: 0x9f,
        loop_offset: None,
    };
    for _ in 0..1_000_000 {
        ensure!(!cancel.load(Ordering::Relaxed), "PSGlib decode cancelled");
        let at = state.cursor;
        let byte = state.read()?;
        let in_substring = state.substring.is_some();
        if let Some((remaining, return_to)) = state.substring.as_mut() {
            *remaining -= 1;
            if *remaining == 0 {
                state.cursor = *return_to;
                state.substring = None;
            }
        }
        match byte {
            0 => {
                ensure!(
                    !in_substring,
                    "PSGlib end inside a substring is unsupported"
                );
                ensure!(
                    state.frame > 0 && !state.writes.is_empty(),
                    "PSGlib stream has no timed writes"
                );
                for value in [0x9f, 0xbf, 0xdf, 0xff] {
                    state.write(value, at)?;
                }
                return Ok(DecodedStream {
                    writes: state.writes,
                    frames: state.frame + 1,
                    spans: spans(state.visited),
                    loop_offset: state.loop_offset,
                    end_offset: at as u32,
                });
            }
            1 => {
                ensure!(
                    !in_substring,
                    "PSGlib loop marker inside a substring is unsupported"
                );
                state.loop_offset = Some(state.cursor as u32);
            }
            2..=7 => bail!("invalid PSGlib command {byte:#04x}"),
            8..=0x37 => {
                ensure!(!in_substring, "nested PSGlib substrings are unsupported");
                let low = state.read()?;
                let high = state.read()?;
                let target = start + usize::from(u16::from_le_bytes([low, high]));
                let len = byte - 4;
                ensure!(
                    target >= start && target + usize::from(len) <= end,
                    "PSGlib substring escapes its mapped stream window"
                );
                state.substring = Some((len, state.cursor));
                state.cursor = target;
            }
            0x38..=0x3f => {
                state.frame += u32::from(byte - 0x37);
                ensure!(state.frame < MAX_FRAMES, "PSGlib frame limit exceeded");
            }
            0x40..=0x7f => {
                ensure!(
                    state.last_latch & 0x10 == 0 && state.last_latch & 0x60 != 0x60,
                    "PSGlib data byte lacks a tone latch"
                );
                state.write(byte, at)?;
            }
            _ => {
                state.last_latch = byte;
                state.write(byte, at)?;
            }
        }
    }
    bail!("PSGlib command limit exceeded")
}

struct Decoder<'a> {
    bytes: &'a [u8],
    start: usize,
    end: usize,
    cursor: usize,
    substring: Option<(u8, usize)>,
    visited: BTreeSet<usize>,
    writes: Vec<FrameWrite>,
    frame: u32,
    last_latch: u8,
    loop_offset: Option<u32>,
}

impl Decoder<'_> {
    fn read(&mut self) -> Result<u8> {
        ensure!(
            self.cursor >= self.start && self.cursor < self.end,
            "PSGlib read escapes its mapped stream window"
        );
        let byte = *self
            .bytes
            .get(self.cursor)
            .context("truncated PSGlib stream")?;
        self.visited.insert(self.cursor);
        self.cursor += 1;
        Ok(byte)
    }

    fn write(&mut self, value: u8, source: usize) -> Result<()> {
        ensure!(
            self.writes.len() < MAX_WRITES,
            "PSGlib write limit exceeded"
        );
        self.writes.push(FrameWrite {
            frame: self.frame,
            value,
            source_offset: source as u32,
        });
        Ok(())
    }
}

fn spans(offsets: BTreeSet<usize>) -> Vec<FileSpan> {
    let mut spans: Vec<FileSpan> = Vec::new();
    for offset in offsets {
        if let Some(last) = spans.last_mut()
            && last.offset + last.byte_len == offset as u32
        {
            last.byte_len += 1;
        } else {
            spans.push(FileSpan {
                offset: offset as u32,
                byte_len: 1,
            });
        }
    }
    spans
}
