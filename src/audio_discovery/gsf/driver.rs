use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};

use super::super::{
    EngineProfile, MAX_ROM_BYTES, SongCandidate,
    tables::{SongDialect, SongTableEntryKind, SongTableInventory},
    word,
};
use super::bootstrap::{self, Callbacks};

#[path = "fingerprints.rs"]
mod fingerprints;
#[path = "patterns.rs"]
mod patterns;
use patterns::Pattern;

const ROM_BASE: u32 = 0x0800_0000;
const SOUND_INFO_PTR: u32 = 0x0300_7FF0;
const SOUND_MAGIC: u32 = 0x6873_6D53;
const SELECTORS: [&[u8]; 2] = [
    &[
        0x00, 0xB5, 0x00, 0x04, 0x07, 0x4A, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x83, 0x88, 0x59,
        0x00, 0xC9, 0x18, 0x89, 0x00, 0x89, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C,
    ],
    &[
        0x00, 0xB5, 0x00, 0x04, 0x07, 0x4B, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x82, 0x88, 0x51,
        0x00, 0x89, 0x18, 0x89, 0x00, 0xC9, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C,
    ],
];

struct Profile {
    name: &'static str,
    init: &'static Pattern,
    vsync: &'static Pattern,
    copy_len: usize,
    copy_sha256: &'static str,
    sound_init_prefix: &'static [u8],
    regions: &'static [fingerprints::Region],
    sound_info_len: usize,
    jump_literal: usize,
}

const PROFILES: [Profile; 2] = [
    Profile {
        name: "stock_mp2k_copy_896_v1",
        regions: fingerprints::BASE,
        sound_info_len: 0x980,
        jump_literal: 0x78,
        init: &patterns::INIT_BASE,
        vsync: &patterns::VSYNC_BASE,
        copy_len: 896,
        copy_sha256: "c4dc8d39b77670492cb88f149b2ef00f83d7f4e07e388f3fb65f6cefc056d2b7",
        sound_init_prefix: &[
            0x30, 0xB5, 0x81, 0xB0, 0x05, 0x1C, 0x00, 0x23, 0x2B, 0x60, 0x23, 0x4A, 0x10, 0x68,
            0x80, 0x21, 0x89, 0x04, 0x08, 0x40, 0x00, 0x28, 0x01, 0xD0,
        ],
    },
    Profile {
        name: "stock_mp2k_copy_2048_extended_v1",
        regions: fingerprints::EXTENDED,
        sound_info_len: 0xFB0,
        jump_literal: 0x9A,
        init: &patterns::INIT_EXTENDED,
        vsync: &patterns::VSYNC_EXTENDED,
        copy_len: 2048,
        copy_sha256: "8e5eb2b70a74c2403971be2a4904f5e5bd7869cd6b42daa617b500f75ea0b7cc",
        sound_init_prefix: &[
            0x30, 0xB5, 0x81, 0xB0, 0x05, 0x1C, 0x00, 0x23, 0x2B, 0x60, 0x2B, 0x49, 0x08, 0x68,
            0x80, 0x22, 0x92, 0x04, 0x10, 0x40, 0x00, 0x28, 0x01, 0xD0,
        ],
    },
];

#[derive(Clone)]
pub(super) struct Driver {
    profile: &'static str,
    callbacks: Callbacks,
    rom_len: usize,
    table_offset: u32,
    song_number: u32,
    aliases: Vec<u32>,
    copy_source: u32,
    copy_destination: u32,
    copy_len: usize,
    witnesses: Vec<Witness>,
}

#[derive(Clone, Serialize)]
struct Witness {
    kind: &'static str,
    offset: usize,
    byte_len: usize,
    sha256: String,
}

pub(super) struct Bootstrap {
    pub(super) patched_rom: Vec<u8>,
    pub(super) entry_address: u32,
    pub(super) song_number_offset: u32,
    pub(super) song_number: u32,
    pub(super) metadata: Value,
}

pub(super) fn inspect(
    bytes: &[u8],
    song: &SongCandidate,
    tables: &[SongTableInventory],
    cancel: &AtomicBool,
) -> Result<Driver> {
    inspect_profiles(bytes, song, tables, &PROFILES, cancel)
}

fn inspect_profiles(
    bytes: &[u8],
    song: &SongCandidate,
    tables: &[SongTableInventory],
    profiles: &[Profile],
    cancel: &AtomicBool,
) -> Result<Driver> {
    check_cancel(cancel)?;
    ensure!(
        bytes.len() >= 0xC0 && bytes.len() < MAX_ROM_BYTES,
        "GSF requires a bounded GBA cartridge with room for its bootstrap"
    );
    ensure!(
        song.engine == EngineProfile::Mp2k && song.evidence.song_table_verified,
        "GSF requires a verified stock MP2k song table; this engine is unsupported"
    );
    let reference = song
        .table_entries
        .iter()
        .min_by_key(|entry| (entry.table_offset, entry.index))
        .context("GSF song has no verified table index")?;
    ensure!(
        song.table_entries
            .iter()
            .all(|entry| entry.table_offset == reference.table_offset),
        "GSF song has ambiguous driver tables"
    );
    let table = tables
        .iter()
        .find(|table| table.table.effective_offset == reference.table_offset)
        .context("GSF song table is unavailable")?;
    ensure!(
        table.dialect == SongDialect::Mp2k,
        "GSF does not support this table dialect"
    );
    let selector = table.selector.effective_offset as usize;
    ensure!(
        selector >= 12
            && selector.is_multiple_of(4)
            && SELECTORS
                .iter()
                .any(|pattern| prefix(bytes, selector, pattern))
            && prefix(bytes, selector + 32, &[1, 0xBC, 0, 0x47]),
        "GSF selector code is not a supported Thumb routine"
    );
    let wrapper = selector - 12;
    ensure!(
        prefix(bytes, wrapper, &[0, 0xB5]) && prefix(bytes, wrapper + 6, &[1, 0xBC, 0, 0x47, 0, 0]),
        "GSF main wrapper is unverified"
    );
    let main = bl_target(bytes, wrapper + 2)?;
    ensure!(
        matches(bytes, main, &patterns::MAIN),
        "GSF SoundMain implementation is unsupported"
    );
    let bx = bl_target(bytes, main + 0x3C)?;
    ensure!(
        bx == bl_target(bytes, main + 0x44)? && prefix(bytes, bx, &[0x18, 0x47]),
        "GSF SoundMain callback bridge is unverified"
    );
    for (offset, value) in [
        (0x6C, SOUND_INFO_PTR),
        (0x70, SOUND_MAGIC),
        (0x78, 0x0400_0006),
        (0x7C, 0x350),
        (0x80, 0x630),
    ] {
        ensure!(
            word(bytes, main + offset) == Some(value),
            "GSF SoundMain literal is unsupported"
        );
    }
    let mut found = Vec::new();
    for profile in profiles {
        for init in (selector.saturating_sub(0x120)..wrapper).step_by(4) {
            check_cancel(cancel)?;
            if matches(bytes, init, profile.init)
                && let Ok(driver) = inspect_profile(bytes, song, table, init, profile, cancel)
            {
                found.push(driver);
            }
        }
    }
    check_cancel(cancel)?;
    ensure!(
        found.len() == 1,
        "GSF has no unique qualified driver bootstrap; custom and unverified MP2k variants are unsupported"
    );
    Ok(found.remove(0))
}

fn inspect_profile(
    bytes: &[u8],
    song: &SongCandidate,
    table: &SongTableInventory,
    init: usize,
    profile: &Profile,
    cancel: &AtomicBool,
) -> Result<Driver> {
    let selector = table.selector.effective_offset as usize;
    let wrapper = selector - 12;
    let main = bl_target(bytes, wrapper + 2)?;
    let verified_regions = fingerprints::verify(bytes, main, init, selector, profile.regions)?;
    let read_literal = |relative| literal(bytes, init + relative).map(|(_, value)| value);
    let copy_source = read_literal(2)?;
    let copy_destination = read_literal(10)?;
    ensure!(
        copy_source == ROM_BASE + main as u32 + 0x85
            && word(bytes, main + 0x74) == Some(copy_destination | 1),
        "GSF mixer relocation is inconsistent"
    );
    ensure!(
        read_literal(12)? == 0x0400_0000 | (profile.copy_len as u32 / 4)
            && iwram(copy_destination, profile.copy_len),
        "GSF mixer relocation exceeds its qualified RAM range"
    );
    let sound_init = bl_target(bytes, init + 0x14)?;
    let mut ram_spans = Vec::new();
    for (address, len) in [
        (copy_destination, profile.copy_len),
        (read_literal(18)?, profile.sound_info_len),
        (read_literal(24)?, 0x100),
        (read_literal(0x42)?, 16),
        (literal(bytes, sound_init + profile.jump_literal)?.1, 0x90),
    ] {
        ensure!(
            iwram(address, len),
            "GSF custom sound state requires an unsupported setup"
        );
        add_ram_span(&mut ram_spans, address, len)?;
    }
    ensure!(
        read_literal(30)?
            == word(
                bytes,
                table.settings_fields.sound_mode.effective_offset as usize
            )
            .context("truncated sound mode")?
            && read_literal(36)?
                == word(
                    bytes,
                    table.settings_fields.player_count.effective_offset as usize
                )
                .context("truncated player count")?
            && read_literal(46)?
                == word(
                    bytes,
                    table.settings_fields.player_table_pointer.effective_offset as usize
                )
                .context("truncated player table")?,
        "GSF initialization and table settings disagree"
    );
    let players = read_literal(36)?;
    let player_address = read_literal(46)?;
    ensure!(
        (1..=32).contains(&players)
            && word(bytes, selector + 36) == Some(player_address)
            && word(bytes, selector + 40) == Some(ROM_BASE + table.table.effective_offset),
        "GSF selector table literals disagree"
    );
    let player_table = rom_offset(bytes, player_address, players as usize * 12)?;
    for at in (player_table..player_table + players as usize * 12).step_by(12) {
        let tracks = u32::from(bytes[at + 8]);
        ensure!(
            (1..=16).contains(&tracks)
                && ram(word(bytes, at).unwrap(), 0x40)
                && ram(word(bytes, at + 4).unwrap(), tracks as usize * 0x50),
            "GSF player state exceeds its qualified RAM range"
        );
        add_ram_span(&mut ram_spans, word(bytes, at).unwrap(), 0x40)?;
        add_ram_span(
            &mut ram_spans,
            word(bytes, at + 4).unwrap(),
            tracks as usize * 0x50,
        )?;
    }
    let copy_at = main + 0x84;
    let copied = bounded(bytes, copy_at, profile.copy_len)?;
    ensure!(
        zeff_firmware::sha256_hex(copied) == profile.copy_sha256,
        "GSF copied mixer is not a qualified implementation"
    );
    let mut witnesses = verified_regions;
    witnesses.extend([
        witness(bytes, "reset", 0, 4)?,
        witness(bytes, "init_and_literals", init, wrapper - init)?,
        witness(bytes, "main_wrapper", wrapper, 12)?,
        witness(bytes, "selector", selector, 44)?,
        witness(bytes, "sound_main", main, 0x84)?,
        witness(bytes, "copied_mixer", copy_at, profile.copy_len)?,
        witness(bytes, "player_table", player_table, players as usize * 12)?,
    ]);
    let prefixes: [(usize, &[u8]); 5] = [
        (0x0E, &[0x0B, 0xDF, 0x70, 0x47]),
        (0x14, profile.sound_init_prefix),
        (
            0x1A,
            &[
                0x70, 0xB5, 0x81, 0xB0, 0x05, 0x1C, 0x30, 0x49, 0x8F, 0x20, 0x08, 0x80, 0x2F, 0x4B,
                0x00, 0x22, 0x1A, 0x80, 0x2F, 0x48, 0x08, 0x21, 0x01, 0x70,
            ],
        ),
        (
            0x20,
            &[
                0x30, 0xB5, 0x03, 0x1C, 0x21, 0x48, 0x05, 0x68, 0x29, 0x68, 0x21, 0x48, 0x81, 0x42,
                0x3A, 0xD1, 0x48, 0x1C, 0x28, 0x60, 0xFF, 0x24, 0x1C, 0x40,
            ],
        ),
        (
            0x3A,
            &[
                0xF0, 0xB5, 0x07, 0x1C, 0x0E, 0x1C, 0x12, 0x06, 0x14, 0x0E, 0x00, 0x2C, 0x2A, 0xD0,
                0x10, 0x2C, 0x00, 0xD9, 0x10, 0x24, 0x15, 0x48, 0x05, 0x68,
            ],
        ),
    ];
    for (relative, expected) in prefixes {
        let target = bl_target(bytes, init + relative)?;
        ensure!(
            prefix(bytes, target, expected),
            "GSF initialization callback is unsupported"
        );
        witnesses.push(witness(bytes, "init_callback", target, expected.len())?);
    }
    if profile.init.bytes.len() == 0x80 {
        ensure!(
            bl_target(bytes, init + 0x6A)? == bl_target(bytes, init + 0x3A)?,
            "GSF extended player setup is inconsistent"
        );
        let memcpy = bl_target(bytes, init + 0x54)?;
        ensure!(
            prefix(
                bytes,
                memcpy,
                &[0x30, 0xB5, 0x05, 0x1C, 0x2C, 0x1C, 0x0B, 0x1C, 0x0F, 0x2A]
            ),
            "GSF extended setup copy is unsupported"
        );
        witnesses.push(witness(bytes, "extended_copy", memcpy, 24)?);
        ensure!(
            iwram(read_literal(0x4E)?, 0x34)
                && rom_offset(bytes, read_literal(0x50)?, 0x34).is_ok()
                && iwram(read_literal(0x5E)?, 0x80)
                && iwram(read_literal(0x62)?, 0x140),
            "GSF extended setup literals are invalid"
        );
        for (address, len) in [
            (read_literal(0x4E)?, 0x34),
            (read_literal(0x5E)?, 0x80),
            (read_literal(0x62)?, 0x140),
        ] {
            add_ram_span(&mut ram_spans, address, len)?;
        }
    }
    let start = bl_target(bytes, selector + 28)?;
    ensure!(
        prefix(bytes, start, &[0xF0, 0xB5]),
        "GSF song-start callback is unsupported"
    );
    witnesses.push(witness(bytes, "song_start", start, 24)?);
    let mut vsyncs = Vec::new();
    for at in (init.saturating_sub(0x1800)..(init + 0x1800).min(bytes.len())).step_by(4) {
        check_cancel(cancel)?;
        if matches(bytes, at, profile.vsync)
            && literal(bytes, at).map(|(_, value)| value).ok() == Some(SOUND_INFO_PTR)
            && literal(bytes, at + 4).map(|(_, value)| value).ok() == Some(SOUND_MAGIC)
        {
            vsyncs.push(at);
        }
    }
    ensure!(
        vsyncs.len() == 1,
        "GSF VBlank callback is ambiguous or unsupported"
    );
    let vsync = vsyncs[0];
    witnesses.push(witness(bytes, "vsync", vsync, profile.vsync.bytes.len())?);
    for (relative, value) in [
        (0, SOUND_INFO_PTR),
        (4, SOUND_MAGIC),
        (0x1A, 0x0400_00BC),
        (0x22, 0x8440_0004),
    ] {
        let (at, actual) = literal(bytes, vsync + relative)?;
        ensure!(actual == value, "GSF VBlank DMA literals are unsupported");
        witnesses.push(witness(bytes, "vsync_literal", at, 4)?);
    }
    let mut aliases = Vec::new();
    for reference in &song.table_entries {
        let entry = table
            .entries
            .iter()
            .find(|entry| entry.index == reference.index)
            .context("GSF table index is absent")?;
        let at = table.table.effective_offset as usize + reference.index as usize * 8;
        ensure!(
            entry.kind == SongTableEntryKind::Song
                && entry.entry.effective_offset as usize == at
                && reference.entry == entry.entry
                && entry.header_address == song.header.canonical_cpu_address
                && entry.player == reference.player
                && u32::from(entry.player) < players
                && word(bytes, at) == Some(entry.header_address)
                && word(bytes, at + 4).map(|value| value as u16) == Some(entry.player),
            "GSF selected song table entry is inconsistent"
        );
        aliases.push(reference.index);
        witnesses.push(witness(bytes, "selected_song_entry", at, 8)?);
    }
    aliases.sort_unstable();
    aliases.dedup();
    let song_number = *aliases.first().context("GSF song has no selected index")?;
    ensure!(
        song_number <= u16::MAX as u32,
        "GSF song number exceeds its driver ABI"
    );
    Ok(Driver {
        profile: profile.name,
        callbacks: Callbacks {
            init: (ROM_BASE + init as u32) | 1,
            init_r0: None,
            select: (ROM_BASE + selector as u32) | 1,
            main: (ROM_BASE + wrapper as u32) | 1,
            prime_main_after_init: false,
            main_in_vblank: false,
            vsync: Some((ROM_BASE + vsync as u32) | 1),
            dma1: None,
            dma2: None,
        },
        rom_len: bytes.len(),
        table_offset: table.table.effective_offset,
        song_number,
        aliases,
        copy_source: copy_source & !1,
        copy_destination,
        copy_len: profile.copy_len,
        witnesses,
    })
}

pub(super) fn build(bytes: &[u8], driver: &Driver, cancel: &AtomicBool) -> Result<Bootstrap> {
    check_cancel(cancel)?;
    ensure!(
        bytes.len() == driver.rom_len,
        "GSF media length changed after driver inspection"
    );
    for expected in &driver.witnesses {
        check_cancel(cancel)?;
        ensure!(
            witness(bytes, expected.kind, expected.offset, expected.byte_len)?.sha256
                == expected.sha256,
            "GSF driver bytes changed after inspection"
        );
    }
    let base = bytes
        .len()
        .checked_add(3)
        .context("GSF bootstrap offset overflow")?
        & !3;
    let code = bootstrap::build(ROM_BASE + base as u32, driver.song_number, driver.callbacks)?;
    ensure!(
        base.checked_add(code.bytes.len())
            .is_some_and(|end| end <= MAX_ROM_BYTES),
        "GSF bootstrap does not fit in cartridge memory"
    );
    let branch_words = (base as i64 - 8) / 4;
    ensure!(
        (-0x80_0000..0x80_0000).contains(&branch_words),
        "GSF reset branch is out of range"
    );
    let branch = 0xEA00_0000 | (branch_words as u32 & 0x00FF_FFFF);
    let mut patched_rom = Vec::with_capacity(base + code.bytes.len());
    for block in bytes.chunks(64 * 1024) {
        check_cancel(cancel)?;
        patched_rom.extend_from_slice(block);
    }
    patched_rom.resize(base, 0);
    patched_rom.extend_from_slice(&code.bytes);
    patched_rom[..4].copy_from_slice(&branch.to_le_bytes());
    let song_number_offset = (base + code.song_literal_offset) as u32;
    let metadata = json!({
        "profile": driver.profile, "bootstrap_version": 1,
        "callbacks": { "init": driver.callbacks.init, "select": driver.callbacks.select, "main": driver.callbacks.main, "vsync": driver.callbacks.vsync },
        "song_table_offset": driver.table_offset, "song_number": driver.song_number, "song_index_aliases": driver.aliases,
        "mixer_copy": { "source_address": driver.copy_source, "destination_address": driver.copy_destination, "byte_len": driver.copy_len },
        "reserved_iwram": { "start_address": 0x0300_7B00u32, "end_address_exclusive": 0x0300_8000u32, "system_stack_top": 0x0300_7F00u32, "irq_stack_top": 0x0300_7FA0u32 },
        "source_witnesses": driver.witnesses,
        "patches": { "reset_offset": 0, "reset_original": word(bytes, 0), "reset_branch": branch, "bootstrap_offset": base, "bootstrap_byte_len": code.bytes.len(), "bootstrap_sha256": zeff_firmware::sha256_hex(&code.bytes), "song_number_offset": song_number_offset, "wait_address": ROM_BASE + base as u32 + code.wait_offset as u32, "irq_address": ROM_BASE + base as u32 + code.irq_offset as u32 },
        "qualification": "Original stock MP2k driver execution with independent initialization and VBlank scheduling; custom drivers and game-side sound effects are unsupported."
    });
    check_cancel(cancel)?;
    Ok(Bootstrap {
        patched_rom,
        entry_address: ROM_BASE,
        song_number_offset,
        song_number: driver.song_number,
        metadata,
    })
}

fn matches(bytes: &[u8], at: usize, pattern: &Pattern) -> bool {
    bounded(bytes, at, pattern.bytes.len()).is_ok_and(|actual| {
        actual
            .iter()
            .zip(pattern.bytes)
            .zip(pattern.mask)
            .all(|((&a, &b), &mask)| a & mask == b)
    })
}

fn prefix(bytes: &[u8], at: usize, expected: &[u8]) -> bool {
    bytes.get(at..at.saturating_add(expected.len())) == Some(expected)
}

fn bounded(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(at..at.checked_add(len).context("GSF span overflow")?)
        .context("GSF driver span exceeds cartridge memory")
}

fn literal(bytes: &[u8], at: usize) -> Result<(usize, u32)> {
    let opcode = u16::from_le_bytes(bounded(bytes, at, 2)?.try_into().unwrap());
    ensure!(
        opcode & 0xF800 == 0x4800,
        "GSF expected a Thumb literal load"
    );
    let offset = ((at + 4) & !3) + usize::from(opcode & 255) * 4;
    Ok((
        offset,
        word(bytes, offset).context("GSF literal exceeds cartridge memory")?,
    ))
}

fn bl_target(bytes: &[u8], at: usize) -> Result<usize> {
    let instruction = word(bytes, at).context("GSF call exceeds cartridge memory")?;
    let first = instruction as u16;
    let second = (instruction >> 16) as u16;
    ensure!(
        first & 0xF800 == 0xF000 && second & 0xF800 == 0xF800,
        "GSF callback is not a Thumb BL"
    );
    let bits = (u32::from(first & 0x7FF) << 12) | (u32::from(second & 0x7FF) << 1);
    let displacement = ((bits << 9) as i32) >> 9;
    let target = usize::try_from(at as i64 + 4 + i64::from(displacement))
        .context("GSF callback address underflow")?;
    bounded(bytes, target, 2)?;
    Ok(target)
}

fn rom_offset(bytes: &[u8], address: u32, len: usize) -> Result<usize> {
    ensure!(address.is_multiple_of(4), "GSF ROM literal is unaligned");
    let at = address
        .checked_sub(ROM_BASE)
        .context("GSF ROM literal is outside cartridge memory")? as usize;
    bounded(bytes, at, len)?;
    Ok(at)
}

fn iwram(address: u32, len: usize) -> bool {
    // Keep the system/IRQ/BIOS stacks and interrupt flags outside all driver state.
    address.is_multiple_of(4)
        && address >= 0x0300_0000
        && u64::from(address) + len as u64 <= 0x0300_7B00
}

fn add_ram_span(spans: &mut Vec<(u32, usize)>, address: u32, len: usize) -> Result<()> {
    ensure!(
        ram(address, len),
        "GSF state overlaps reserved bootstrap stack memory"
    );
    let end = u64::from(address) + len as u64;
    ensure!(
        spans.iter().all(|&(other, size)| end <= u64::from(other)
            || u64::from(address) >= u64::from(other) + size as u64),
        "GSF mutable driver state overlaps another state or code region"
    );
    spans.push((address, len));
    Ok(())
}

fn ram(address: u32, len: usize) -> bool {
    iwram(address, len)
        || (address.is_multiple_of(4)
            && address >= 0x0200_0000
            && u64::from(address) + len as u64 <= 0x0204_0000)
}

fn witness(bytes: &[u8], kind: &'static str, offset: usize, byte_len: usize) -> Result<Witness> {
    Ok(Witness {
        kind,
        offset,
        byte_len,
        sha256: zeff_firmware::sha256_hex(bounded(bytes, offset, byte_len)?),
    })
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("GSF export cancelled");
    }
    Ok(())
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) fn synthetic_driver(bytes: &[u8], song_number: u32) -> Result<Driver> {
    tests::fixture_driver(bytes, song_number)
}
