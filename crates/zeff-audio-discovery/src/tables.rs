use serde::Serialize;

use super::{Budget, RomSpan, ScanStop, rom_pointer, word};

const MAX_TABLES: usize = 8;
const MAX_ENTRY_SLOTS_PER_TABLE: usize = 4_096;
const MAX_TOTAL_ENTRY_SLOTS: usize = 16_384;
const SONG_ID_SETTINGS_LEN: usize = 0x70;

const SONG_SELECT_V1: [u8; 30] = [
    0x00, 0xB5, 0x00, 0x04, 0x07, 0x4A, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x83, 0x88, 0x59, 0x00,
    0xC9, 0x18, 0x89, 0x00, 0x89, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C, 0x00, 0xF0,
];
const SONG_SELECT_V2: [u8; 30] = [
    0x00, 0xB5, 0x00, 0x04, 0x07, 0x4B, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x82, 0x88, 0x51, 0x00,
    0x89, 0x18, 0x89, 0x00, 0xC9, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C, 0x00, 0xF0,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SongTableInventory {
    pub selector: RomSpan,
    pub settings: RomSpan,
    pub settings_fields: SettingsFields,
    pub dialect: SongDialect,
    pub table: RomSpan,
    pub entries: Vec<SongTableEntry>,
    pub boundary: SongTableBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SettingsFields {
    pub sound_mode: RomSpan,
    pub player_count: RomSpan,
    pub player_table_pointer: RomSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SongDialect {
    Mp2k,
    SongIdHeader,
}

#[derive(Clone, Copy)]
struct SettingsEvidence {
    span: RomSpan,
    fields: SettingsFields,
    dialect: SongDialect,
    players: u32,
    player_table: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SongTableEntry {
    pub index: u32,
    pub entry: RomSpan,
    pub header_address: u32,
    pub track_count: u8,
    pub player: u16,
    pub kind: SongTableEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SongTableEntryKind {
    Null,
    Placeholder,
    Song,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SongTableBoundary {
    NullTerminator { entry: RomSpan },
    InvalidPlayer { entry: RomSpan, player: u16 },
    InvalidHeaderPointer { entry: RomSpan, address: u32 },
    MediaEnd { effective_offset: u32 },
}

pub(crate) fn discover(
    bytes: &[u8],
    inventories: &mut Vec<SongTableInventory>,
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    if bytes.len() < SONG_SELECT_V1.len() {
        return Ok(());
    }

    let song_id_settings = find_song_id_settings(bytes, budget)?;
    let mut total_entry_slots = inventories
        .iter()
        .map(|inventory| inventory.entries.len())
        .sum::<usize>();
    for selector_offset in (0..=bytes.len() - SONG_SELECT_V1.len()).step_by(2) {
        budget.charge()?;
        let signature = &bytes[selector_offset..selector_offset + 28];
        if (signature != &SONG_SELECT_V1[..28] && signature != &SONG_SELECT_V2[..28])
            || thumb_bl_target(bytes, selector_offset + 28).is_none()
        {
            continue;
        }
        // The table LDR at +6 uses the aligned Thumb PC, which permits a +40 or +42 literal.
        let literal_offset = ((selector_offset + 10) & !3) + 32;
        let selector_end = literal_offset + 4;
        if selector_end > bytes.len() || selector_offset < 32 {
            continue;
        }
        let literal_address = word(bytes, literal_offset).expect("bounded selector evidence");
        let Some(literal_table) = rom_pointer(bytes, literal_address, 8, 4) else {
            continue;
        };
        let player_literal_offset = ((selector_offset + 8) & !3) + 28;
        let Some(player_table) = word(bytes, player_literal_offset)
            .and_then(|address| rom_pointer(bytes, address, 12, 4))
        else {
            continue;
        };
        let mut settings = None;
        if let Some(main_offset) = find_main(bytes, selector_offset, budget)? {
            for distance in [16, 32] {
                budget.charge()?;
                if let Some(offset) = main_offset
                    .checked_sub(distance)
                    .filter(|&offset| valid_settings(bytes, offset, literal_table))
                {
                    if word(bytes, offset + 8)
                        .and_then(|address| rom_pointer(bytes, address, 12, 4))
                        != Some(player_table)
                    {
                        continue;
                    }
                    settings = Some(SettingsEvidence {
                        span: RomSpan::new(offset, 12),
                        fields: SettingsFields {
                            sound_mode: RomSpan::new(offset, 4),
                            player_count: RomSpan::new(offset + 4, 4),
                            player_table_pointer: RomSpan::new(offset + 8, 4),
                        },
                        dialect: SongDialect::Mp2k,
                        players: word(bytes, offset + 4).expect("validated settings"),
                        player_table,
                    });
                    break;
                }
            }
        }
        if settings.is_none() {
            settings = song_id_settings.iter().copied().find(|settings| {
                settings.player_table == player_table
                    && settings.player_table + settings.players as usize * 12 == literal_table
            });
        }
        let Some(settings) = settings else {
            continue;
        };
        if inventories
            .iter()
            .any(|inventory| inventory.table.effective_offset as usize == literal_table)
        {
            continue;
        }
        if inventories.len() >= MAX_TABLES {
            return Err(ScanStop::InventoryLimit);
        }

        let remaining_entry_slots = MAX_TOTAL_ENTRY_SLOTS - total_entry_slots;
        let Some((entries, boundary)) = read_table(
            bytes,
            literal_table,
            settings.players,
            settings.dialect,
            remaining_entry_slots,
            budget,
        )?
        else {
            continue;
        };
        total_entry_slots += entries.len();
        inventories.push(SongTableInventory {
            selector: RomSpan::new(selector_offset, selector_end - selector_offset),
            settings: settings.span,
            settings_fields: settings.fields,
            dialect: settings.dialect,
            table: RomSpan::new(literal_table, entries.len() * 8),
            entries,
            boundary,
        });
    }
    Ok(())
}

fn thumb_bl_target(bytes: &[u8], offset: usize) -> Option<usize> {
    let instruction = word(bytes, offset)?;
    let first = instruction as u16;
    let second = (instruction >> 16) as u16;
    if first & 0xF800 != 0xF000 || second & 0xF800 != 0xF800 {
        return None;
    }
    let encoded = (u32::from(first & 0x7FF) << 12) | (u32::from(second & 0x7FF) << 1);
    let displacement = ((encoded << 9) as i32) >> 9;
    let target = (offset as i64 + 4).checked_add(i64::from(displacement))?;
    let target = usize::try_from(target).ok()?;
    bytes.get(target..target.checked_add(2)?)?;
    target.is_multiple_of(2).then_some(target)
}

fn ram_pointer(address: u32, len: usize, align: usize) -> bool {
    let end = u64::from(address) + len as u64;
    (address as usize).is_multiple_of(align)
        && ((address >= 0x0200_0000 && end <= 0x0204_0000)
            || (address >= 0x0300_0000 && end <= 0x0300_8000))
}

fn find_song_id_settings(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<SettingsEvidence>, ScanStop> {
    let mut matches = Vec::new();
    if bytes.len() < SONG_ID_SETTINGS_LEN {
        return Ok(matches);
    }
    for offset in (0..=bytes.len() - SONG_ID_SETTINGS_LEN).step_by(4) {
        budget.charge()?;
        if word(bytes, offset + 4) != Some(0x0400_0200) {
            continue;
        }
        let read = |relative| word(bytes, offset + relative).expect("bounded literal pool");
        if !ram_pointer(read(0), 4, 4)
            || ![
                (0x08, 0x0400_0084),
                (0x0C, 0x0400_0082),
                (0x14, 0x0400_0089),
                (0x18, 0x0400_0063),
                (0x1C, 0x0400_0080),
                (0x68, 0x0400_00D4),
            ]
            .into_iter()
            .all(|(field, expected)| read(field) == expected)
        {
            continue;
        }
        let valid_copies = [0x20, 0x30, 0x40].into_iter().all(|field| {
            let destination = read(field + 4);
            let source = read(field + 8);
            let control = read(field + 12);
            let halfwords = control & 0xFFFF;
            ram_pointer(read(field), 4, 4)
                && destination & 1 == 1
                && source & 1 == 1
                && halfwords > 0
                && halfwords <= 0x800
                && control & 0xFFFF_0000 == 0x8000_0000
                && (0x0300_0000..0x0300_8000).contains(&(destination & !1))
                && ram_pointer(destination & !1, halfwords as usize * 2, 2)
                && rom_pointer(bytes, source & !1, halfwords as usize * 2, 2).is_some()
        });
        let players = read(0x58);
        let mode = read(0x5C);
        if !(1..=32).contains(&players) {
            continue;
        }
        let Some(player_table) = rom_pointer(bytes, read(0x6C), players as usize * 12, 4) else {
            continue;
        };
        if !valid_copies
            || mode >> 24 > 2
            || !valid_sound_mode(mode & 0x00FF_FFFF)
            || mode & 0xF00 == 0
            || !(0..players as usize).all(|index| {
                let slot = player_table + index * 12;
                ram_pointer(word(bytes, slot).expect("bounded player table"), 44, 4)
                    && ram_pointer(word(bytes, slot + 4).expect("bounded player table"), 80, 4)
                    && (1..=24).contains(&u16::from_le_bytes(
                        bytes[slot + 8..slot + 10].try_into().expect("two bytes"),
                    ))
                    && u16::from_le_bytes(
                        bytes[slot + 10..slot + 12].try_into().expect("two bytes"),
                    ) <= 1
            })
        {
            continue;
        }
        if matches.len() == MAX_TABLES {
            return Err(ScanStop::InventoryLimit);
        }
        matches.push(SettingsEvidence {
            span: RomSpan::new(offset, SONG_ID_SETTINGS_LEN),
            fields: SettingsFields {
                sound_mode: RomSpan::new(offset + 0x5C, 4),
                player_count: RomSpan::new(offset + 0x58, 4),
                player_table_pointer: RomSpan::new(offset + 0x6C, 4),
            },
            dialect: SongDialect::SongIdHeader,
            players,
            player_table,
        });
    }
    Ok(matches)
}

fn find_main(
    bytes: &[u8],
    selector_offset: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<usize>, ScanStop> {
    let mut main = None;
    for offset in (selector_offset - 31..=selector_offset)
        .rev()
        .filter(|offset| offset.is_multiple_of(2))
    {
        budget.charge()?;
        if bytes.get(offset..offset + 2) == Some([0x00, 0xB5].as_slice()) {
            main = Some(offset);
        }
    }
    Ok(main)
}

fn valid_settings(bytes: &[u8], offset: usize, literal_table: usize) -> bool {
    let Some(raw) = word(bytes, offset) else {
        return false;
    };
    let Some(song_levels) = word(bytes, offset + 4) else {
        return false;
    };
    let Some(base_address) = word(bytes, offset + 8) else {
        return false;
    };
    if raw & 0xFF00_0000 != 0 || song_levels == 0 || song_levels >= 256 {
        return false;
    }

    if !valid_sound_mode(raw) {
        return false;
    }

    let Some(base_offset) = rom_pointer(bytes, base_address, 0, 4) else {
        return false;
    };
    base_offset
        .checked_add(song_levels as usize * 12)
        .is_some_and(|computed| computed == literal_table)
}

fn valid_sound_mode(raw: u32) -> bool {
    let polyphony = (raw & 0x0000_0F00) >> 8;
    let main_volume = (raw & 0x0000_F000) >> 12;
    let sampling_rate_index = (raw & 0x000F_0000) >> 16;
    let dac_bits = 17 - ((raw & 0x00F0_0000) >> 20);
    main_volume != 0
        && polyphony < 13
        && (1..=12).contains(&sampling_rate_index)
        && (6..=9).contains(&dac_bits)
}

fn read_table(
    bytes: &[u8],
    table_offset: usize,
    song_levels: u32,
    dialect: SongDialect,
    remaining_entry_slots: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<(Vec<SongTableEntry>, SongTableBoundary)>, ScanStop> {
    let mut entries = Vec::new();
    let mut pending_nulls: Vec<SongTableEntry> = Vec::new();
    let mut song_count = 0;
    let boundary = loop {
        budget.charge()?;
        let slot_index = entries.len() + pending_nulls.len();
        if slot_index >= MAX_ENTRY_SLOTS_PER_TABLE || slot_index >= remaining_entry_slots {
            return Err(ScanStop::InventoryLimit);
        }
        let Some(entry_offset) = table_offset.checked_add(slot_index * 8) else {
            return Ok(None);
        };
        let Some(slot) = bytes.get(entry_offset..entry_offset.saturating_add(8)) else {
            if let Some(first_null) = pending_nulls.first() {
                break SongTableBoundary::NullTerminator {
                    entry: first_null.entry,
                };
            }
            break SongTableBoundary::MediaEnd {
                effective_offset: entry_offset.min(u32::MAX as usize) as u32,
            };
        };
        let header_address = u32::from_le_bytes(slot[..4].try_into().expect("four bytes"));
        let player = u16::from_le_bytes(slot[4..6].try_into().expect("two bytes"));
        let entry_span = RomSpan::new(entry_offset, 8);
        if u32::from(player) >= song_levels {
            if let Some(first_null) = pending_nulls.first() {
                break SongTableBoundary::NullTerminator {
                    entry: first_null.entry,
                };
            }
            break SongTableBoundary::InvalidPlayer {
                entry: entry_span,
                player,
            };
        }
        if header_address == 0 {
            pending_nulls.push(SongTableEntry {
                index: slot_index as u32,
                entry: entry_span,
                header_address,
                track_count: 0,
                player,
                kind: SongTableEntryKind::Null,
            });
            continue;
        }
        let Some(header_offset) = rom_pointer(bytes, header_address, 8, 4) else {
            if let Some(first_null) = pending_nulls.first() {
                break SongTableBoundary::NullTerminator {
                    entry: first_null.entry,
                };
            }
            break SongTableBoundary::InvalidHeaderPointer {
                entry: entry_span,
                address: header_address,
            };
        };

        entries.append(&mut pending_nulls);
        let track_count = bytes[header_offset];
        let kind = classify_header(bytes, header_offset, track_count, dialect);
        song_count += usize::from(kind == SongTableEntryKind::Song);
        entries.push(SongTableEntry {
            index: slot_index as u32,
            entry: entry_span,
            header_address,
            track_count,
            player,
            kind,
        });
    };

    Ok((song_count != 0).then_some((entries, boundary)))
}

fn classify_header(
    bytes: &[u8],
    offset: usize,
    track_count: u8,
    dialect: SongDialect,
) -> SongTableEntryKind {
    if track_count == 0 && bytes[offset + 1] == 0 {
        return SongTableEntryKind::Placeholder;
    }
    if !(1..=24).contains(&track_count)
        || (bytes[offset + 1] != 0 && dialect != SongDialect::SongIdHeader)
    {
        return SongTableEntryKind::Unresolved;
    }
    let header_len = 8 + usize::from(track_count) * 4;
    if bytes
        .get(offset..offset.saturating_add(header_len))
        .is_none()
    {
        return SongTableEntryKind::Unresolved;
    }
    let Some(bank_address) = word(bytes, offset + 4) else {
        return SongTableEntryKind::Unresolved;
    };
    if rom_pointer(bytes, bank_address, 12, 4).is_none() {
        return SongTableEntryKind::Unresolved;
    }
    let tracks_valid = (0..usize::from(track_count)).all(|track| {
        word(bytes, offset + 8 + track * 4)
            .and_then(|address| rom_pointer(bytes, address, 1, 1))
            .is_some()
    });
    if tracks_valid {
        SongTableEntryKind::Song
    } else {
        SongTableEntryKind::Unresolved
    }
}
#[cfg(test)]
#[path = "tables/tests.rs"]
mod tests;
