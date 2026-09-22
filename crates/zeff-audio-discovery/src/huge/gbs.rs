use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;

use super::catalog::{HugeSong, MAX_VALIDATION_FRAMES, required_validation_frames};
use super::discovery::{self, BoundSong};
use super::isolation;
use crate::tracker::FileSpan;

const LOAD: u16 = 0x0400;
const END: u16 = 0x8000;
const HEADER_LEN: usize = 0x70;

#[derive(Debug, Serialize)]
pub struct ExperimentalGbs {
    #[serde(skip)]
    pub bytes: Vec<u8>,
    pub source_sha256: String,
    pub translation_delta: u16,
    pub copied_source_spans: Vec<FileSpan>,
    pub copied_target_spans: Vec<FileSpan>,
    pub descriptor: u16,
    pub driver_start: u16,
    pub driver_end: u16,
    pub init_wrapper: FileSpan,
    pub play_wrapper: FileSpan,
    pub init_call: u16,
    pub update_call: u16,
}

pub fn build(bytes: &[u8], selected: &HugeSong, cancel: &AtomicBool) -> Result<ExperimentalGbs> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "experimental hUGE GBS build cancelled"
    );
    let source_sha256 = zeff_firmware::sha256_hex(bytes);
    ensure!(
        source_sha256 == selected.source_sha256,
        "hUGE GBS source hash no longer matches its selection"
    );
    let validation_frames = required_validation_frames(selected.bound.song.loop_ticks)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS recurrence budget overflows"))?;
    ensure!(
        validation_frames == selected.validation_frames
            && validation_frames <= MAX_VALIDATION_FRAMES,
        "hUGE GBS validation budget no longer matches its selection"
    );
    let report = discovery::discover(bytes, Default::default(), cancel)
        .map_err(|stop| anyhow::anyhow!("hUGE GBS rediscovery stopped: {stop:?}"))?;
    let matches = report
        .bound
        .iter()
        .filter(|bound| bound.song.descriptor == selected.bound.song.descriptor)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1 && matches[0] == &selected.bound,
        "hUGE GBS selection no longer has one exact descriptor binding"
    );
    let mut budget = crate::Budget {
        cancel,
        remaining: crate::MAX_SCAN_WORK,
    };
    ensure!(
        isolation::bootstrap_matches(bytes, &selected.bound, &mut budget)
            .map_err(|stop| anyhow::anyhow!("hUGE GBS bootstrap validation stopped: {stop:?}"))?,
        "hUGE GBS source does not match the required isolated bootstrap"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "experimental hUGE GBS build cancelled"
    );

    let copied_source_spans = copied_spans(&selected.bound)?;
    let minimum = copied_source_spans
        .iter()
        .map(|span| span.offset)
        .min()
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS selection has no source spans"))?;
    let translation_delta = LOAD.saturating_sub(u16::try_from(minimum)?);
    let copied_target_spans = copied_source_spans
        .iter()
        .map(|span| relocate_span(*span, translation_delta))
        .collect::<Result<Vec<_>>>()?;
    let descriptor = relocate_address(selected.bound.song.descriptor.offset, translation_delta)?;
    let driver_start = relocate_address(selected.bound.evidence.driver.offset, translation_delta)?;
    let driver_end = relocate_end(
        selected
            .bound
            .evidence
            .driver
            .offset
            .checked_add(selected.bound.evidence.driver.byte_len)
            .ok_or_else(|| anyhow::anyhow!("hUGE GBS driver span overflows"))?,
        translation_delta,
    )?;
    let init = init_code(descriptor, driver_start);
    let play = play_code(selected.bound.evidence.update_address, translation_delta)?;
    let (init_wrapper, play_wrapper) = wrappers(&copied_target_spans, init.len(), play.len())?;

    let mut program = vec![0; usize::from(END - LOAD)];
    for (source, target) in copied_source_spans.iter().zip(&copied_target_spans) {
        copy_span(bytes, &mut program, *source, *target)?;
    }
    patch_driver(
        &mut program,
        driver_start,
        selected.bound.evidence.ram_address,
    )?;
    patch_song_pointers(
        bytes,
        &mut program,
        &selected.bound,
        translation_delta,
        &copied_source_spans,
    )?;
    write_program(&mut program, init_wrapper.offset, &init)?;
    write_program(&mut program, play_wrapper.offset, &play)?;
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "experimental hUGE GBS build cancelled"
    );

    let mut output = vec![0; HEADER_LEN];
    output[..6].copy_from_slice(b"GBS\x01\x01\x01");
    output[6..8].copy_from_slice(&LOAD.to_le_bytes());
    output[8..10].copy_from_slice(&(init_wrapper.offset as u16).to_le_bytes());
    output[10..12].copy_from_slice(&(play_wrapper.offset as u16).to_le_bytes());
    output[12..14].copy_from_slice(&0xfff0_u16.to_le_bytes());
    output[0x10..0x21].copy_from_slice(b"hUGE experimental");
    output.extend(program);
    Ok(ExperimentalGbs {
        bytes: output,
        source_sha256,
        translation_delta,
        copied_source_spans,
        copied_target_spans,
        descriptor,
        driver_start,
        driver_end,
        init_wrapper,
        play_wrapper,
        init_call: init_wrapper.offset as u16 + 23,
        update_call: play_wrapper.offset as u16,
    })
}

fn copied_spans(selected: &BoundSong) -> Result<Vec<FileSpan>> {
    let mut spans = selected.song.spans.clone();
    spans.push(selected.evidence.driver);
    spans.sort_by_key(|span| span.offset);
    for pair in spans.windows(2) {
        ensure!(
            pair[0].offset + pair[0].byte_len <= pair[1].offset,
            "hUGE GBS approved source spans overlap"
        );
    }
    for span in &spans {
        let end = span
            .offset
            .checked_add(span.byte_len)
            .ok_or_else(|| anyhow::anyhow!("hUGE GBS source span overflows"))?;
        ensure!(
            span.byte_len != 0 && end <= u32::from(END),
            "hUGE GBS source span is invalid"
        );
    }
    Ok(spans)
}

fn relocate_span(span: FileSpan, delta: u16) -> Result<FileSpan> {
    let offset = relocate_address(span.offset, delta)?;
    let end = u32::from(offset)
        .checked_add(span.byte_len)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS relocated span overflows"))?;
    ensure!(
        end <= u32::from(END),
        "hUGE GBS relocated span exceeds $7FFF"
    );
    Ok(FileSpan {
        offset: u32::from(offset),
        byte_len: span.byte_len,
    })
}

fn relocate_address(address: u32, delta: u16) -> Result<u16> {
    let relocated = address
        .checked_add(u32::from(delta))
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS relocation overflows"))?;
    ensure!(
        (u32::from(LOAD)..u32::from(END)).contains(&relocated),
        "hUGE GBS address lies outside its load image"
    );
    Ok(relocated as u16)
}

fn relocate_end(address: u32, delta: u16) -> Result<u16> {
    let relocated = address
        .checked_add(u32::from(delta))
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS relocation overflows"))?;
    ensure!(
        relocated <= u32::from(END),
        "hUGE GBS address exceeds $8000"
    );
    Ok(relocated as u16)
}

fn wrappers(spans: &[FileSpan], init_len: usize, play_len: usize) -> Result<(FileSpan, FileSpan)> {
    let needed = init_len
        .checked_add(play_len)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS wrapper length overflows"))?;
    let mut cursor = u32::from(LOAD);
    for span in spans {
        if cursor + needed as u32 <= span.offset {
            return wrapper_pair(cursor, init_len, play_len);
        }
        cursor = cursor.max(span.offset + span.byte_len);
    }
    if cursor + needed as u32 <= u32::from(END) {
        return wrapper_pair(cursor, init_len, play_len);
    }
    anyhow::bail!("hUGE GBS has no safe wrapper gap")
}

fn wrapper_pair(start: u32, init_len: usize, play_len: usize) -> Result<(FileSpan, FileSpan)> {
    let init = FileSpan {
        offset: start,
        byte_len: u32::try_from(init_len)?,
    };
    let play = FileSpan {
        offset: start + init.byte_len,
        byte_len: u32::try_from(play_len)?,
    };
    ensure!(
        play.offset + play.byte_len <= u32::from(END),
        "hUGE GBS wrapper exceeds $7FFF"
    );
    Ok((init, play))
}

fn copy_span(bytes: &[u8], program: &mut [u8], source: FileSpan, target: FileSpan) -> Result<()> {
    let source_start = source.offset as usize;
    let source_end = source_start
        .checked_add(source.byte_len as usize)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS source copy overflows"))?;
    let target_start = target.offset as usize - usize::from(LOAD);
    let target_end = target_start
        .checked_add(target.byte_len as usize)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS target copy overflows"))?;
    program
        .get_mut(target_start..target_end)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS target copy exceeds image"))?
        .copy_from_slice(
            bytes
                .get(source_start..source_end)
                .ok_or_else(|| anyhow::anyhow!("hUGE GBS source copy exceeds source"))?,
        );
    Ok(())
}

fn patch_driver(program: &mut [u8], start: u16, ram: u16) -> Result<()> {
    for index in 0..discovery::reference::CODE.len() {
        let at = usize::from(start - LOAD) + index;
        *program
            .get_mut(at)
            .ok_or_else(|| anyhow::anyhow!("hUGE GBS relocated driver exceeds image"))? =
            discovery::reference::relocated_byte(index, start, ram);
    }
    Ok(())
}

fn patch_song_pointers(
    bytes: &[u8],
    program: &mut [u8],
    selected: &BoundSong,
    delta: u16,
    spans: &[FileSpan],
) -> Result<()> {
    let descriptor = selected.song.descriptor.offset as usize;
    for field in (1..21).step_by(2) {
        let pointer = read_word(bytes, descriptor + field)?;
        if pointer != 0 {
            ensure_descriptor_edge(selected, field, pointer, spans)?;
            write_word(
                program,
                relocate_address((descriptor + field) as u32, delta)?,
                relocate_address(u32::from(pointer), delta)?,
            )?;
        }
    }
    let mut patched_orders = BTreeSet::new();
    let count = selected.song.order_count as usize * 2;
    for field in [3, 5, 7, 9] {
        let orders = read_word(bytes, descriptor + field)?;
        if !patched_orders.insert(orders) {
            continue;
        }
        for index in (0..count).step_by(2) {
            let pattern = read_word(bytes, usize::from(orders) + index)?;
            ensure!(
                has_span(spans, u32::from(pattern), 192),
                "hUGE GBS order pointer leaves approved pattern data"
            );
            write_word(
                program,
                relocate_address(u32::from(orders) + index as u32, delta)?,
                relocate_address(u32::from(pattern), delta)?,
            )?;
        }
    }
    Ok(())
}

fn ensure_descriptor_edge(
    selected: &BoundSong,
    field: usize,
    pointer: u16,
    spans: &[FileSpan],
) -> Result<()> {
    let valid = match field {
        1 => has_span(spans, u32::from(pointer), 1),
        3 | 5 | 7 | 9 => has_span(
            spans,
            u32::from(pointer),
            selected.song.order_count as u32 * 2,
        ),
        11 | 13 | 15 => selected
            .song
            .instruments
            .iter()
            .filter(|instrument| {
                matches!((field, instrument.channel), (11, 0 | 1) | (13, 2) | (15, 3))
            })
            .any(|instrument| {
                let expected = u32::from(pointer) + u32::from(instrument.id - 1) * 6;
                instrument.source.offset == expected && has_span(spans, expected, 6)
            }),
        19 => selected.song.instruments.iter().any(|instrument| {
            let Some(wave) = instrument.wave else {
                return false;
            };
            wave.offset == u32::from(pointer) + u32::from(instrument.data[2]) * 16
                && has_span(spans, wave.offset, 16)
        }),
        _ => false,
    };
    ensure!(
        valid,
        "hUGE GBS descriptor pointer leaves approved song data"
    );
    Ok(())
}

fn has_span(spans: &[FileSpan], offset: u32, byte_len: u32) -> bool {
    spans
        .iter()
        .any(|span| span.offset == offset && span.byte_len == byte_len)
}

fn read_word(bytes: &[u8], at: usize) -> Result<u16> {
    let word = bytes
        .get(at..at + 2)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS pointer exceeds source"))?;
    Ok(u16::from_le_bytes([word[0], word[1]]))
}

fn write_word(program: &mut [u8], address: u16, value: u16) -> Result<()> {
    write_program(program, u32::from(address), &value.to_le_bytes())
}

fn write_program(program: &mut [u8], address: u32, code: &[u8]) -> Result<()> {
    let start = usize::try_from(address)?
        .checked_sub(usize::from(LOAD))
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS write precedes image"))?;
    let end = start
        .checked_add(code.len())
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS write overflows"))?;
    program
        .get_mut(start..end)
        .ok_or_else(|| anyhow::anyhow!("hUGE GBS write exceeds image"))?
        .copy_from_slice(code);
    Ok(())
}

fn init_code(descriptor: u16, driver: u16) -> Vec<u8> {
    let mut code = vec![
        0xf3, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x26, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0xff, 0xe0,
        0x25, 0x3e, 0x77, 0xe0, 0x24, 0x21,
    ];
    code.extend_from_slice(&descriptor.to_le_bytes());
    code.push(0xcd);
    code.extend_from_slice(&driver.to_le_bytes());
    code.push(0xc9);
    code
}

fn play_code(update: u16, delta: u16) -> Result<Vec<u8>> {
    let update = relocate_address(u32::from(update), delta)?;
    Ok(vec![0xcd, update as u8, (update >> 8) as u8, 0xc9])
}

#[cfg(test)]
mod tests;
