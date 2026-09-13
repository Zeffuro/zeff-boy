use std::mem::size_of;
use std::sync::atomic::AtomicBool;

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, rom_pointer, word};

mod assets;
mod bootstrap;
mod data;
mod driver;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod signatures;
#[cfg(test)]
mod tests;

const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALIDATION_WORK: u64 = 100_000_000;
const MAX_EVENTS: usize = 16_384;
const MAX_SPANS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DescriptorMidiSong {
    /// The sparse native selector table, including its null slots.
    pub root: RomSpan,
    /// The selected native descriptor; `midi` contains the complete SMF.
    pub header: RomSpan,
    pub midi: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub tracks: u16,
    pub notes: u32,
    pub instruments: u16,
    pub samples: u16,
    pub native: DescriptorMidiNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DescriptorMidiNativeProfile {
    pub init: RomSpan,
    pub configure: RomSpan,
    pub handoff: RomSpan,
    pub play: RomSpan,
    pub descriptor_play: RomSpan,
    pub update: RomSpan,
    pub dma_irq: RomSpan,
    pub song_table: RomSpan,
    pub player_table: RomSpan,
    pub player_config: RomSpan,
    pub bank_table: RomSpan,
    pub players: Vec<DescriptorMidiPlayer>,
    pub setup_spans: Vec<RomSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DescriptorMidiPlayer {
    pub state: u32,
    pub voice_state: u32,
    pub channels: u8,
}

pub struct PreparedDescriptorMidiRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_rom() -> Vec<u8> {
    fixture::rom()
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}
type ReadResult<T> = Result<T, ReadError>;

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<DescriptorMidiSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let mut retained = songs
        .capacity()
        .saturating_mul(size_of::<DescriptorMidiSong>());
    for song in songs.iter() {
        retained = retained.saturating_add(owned_bytes(song));
    }
    let mut unresolved = false;
    for native in driver::recognize(bytes, budget)? {
        for index in 0..native.song_table.byte_len as usize / 8 {
            budget.charge()?;
            let slot = native.song_table.effective_offset as usize + index * 8;
            if word(bytes, slot) == Some(0) {
                continue;
            }
            let song = match data::parse_song(bytes, &native, index as u16, budget) {
                Ok(song) => song,
                Err(ReadError::Invalid) => {
                    unresolved = true;
                    continue;
                }
                Err(ReadError::Stop(stop)) => return Err(stop),
            };
            if bootstrap::placement(bytes).is_none() {
                unresolved = true;
                continue;
            }
            if songs.len() >= max_candidates {
                return Err(ScanStop::CandidateLimit);
            }
            push_song(songs, song, &mut retained)?;
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

fn push_song(
    songs: &mut Vec<DescriptorMidiSong>,
    song: DescriptorMidiSong,
    retained: &mut usize,
) -> Result<(), ScanStop> {
    let owned = owned_bytes(&song);
    let available = MAX_RETAINED_BYTES
        .checked_sub(*retained)
        .and_then(|n| n.checked_sub(owned))
        .ok_or(ScanStop::InventoryLimit)?;
    let mut added = 0;
    if songs.len() == songs.capacity() {
        if available < size_of::<DescriptorMidiSong>() {
            return Err(ScanStop::InventoryLimit);
        }
        let previous = songs.capacity();
        songs
            .try_reserve_exact(1)
            .map_err(|_| ScanStop::InventoryLimit)?;
        added = (songs.capacity() - previous) * size_of::<DescriptorMidiSong>();
        if added > available {
            return Err(ScanStop::InventoryLimit);
        }
    }
    *retained += owned + added;
    songs.push(song);
    Ok(())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &DescriptorMidiSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedDescriptorMidiRom> {
    revalidate(bytes, song, cancel)?;
    bootstrap::build(bytes, song)
}

pub fn midi_bytes(
    bytes: &[u8],
    song: &DescriptorMidiSong,
    cancel: &AtomicBool,
) -> AnyResult<Vec<u8>> {
    revalidate(bytes, song, cancel)?;
    let at = song.midi.effective_offset as usize;
    Ok(bytes[at..at + song.midi.byte_len as usize].to_vec())
}

fn revalidate(bytes: &[u8], song: &DescriptorMidiSong, cancel: &AtomicBool) -> AnyResult<()> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "Descriptor MIDI source exceeds the ROM size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let profiles = driver::recognize(bytes, &mut budget)
        .map_err(|e| anyhow::anyhow!("Descriptor MIDI driver validation stopped: {e:?}"))?;
    ensure!(
        profiles.contains(&song.native),
        "Descriptor MIDI driver no longer matches"
    );
    let current = data::parse_song(bytes, &song.native, song.index, &mut budget)
        .map_err(|e| anyhow::anyhow!("Descriptor MIDI validation failed: {e:?}"))?;
    ensure!(
        current == *song,
        "Descriptor MIDI selection no longer matches its source"
    );
    Ok(())
}

fn owned_bytes(song: &DescriptorMidiSong) -> usize {
    song.title
        .capacity()
        .saturating_add(song.mapped_spans.capacity() * size_of::<RomSpan>())
        .saturating_add(song.native.setup_spans.capacity() * size_of::<RomSpan>())
        .saturating_add(song.native.players.capacity() * size_of::<DescriptorMidiPlayer>())
        .saturating_add(song.warnings.capacity() * size_of::<String>())
        .saturating_add(song.warnings.iter().map(String::capacity).sum::<usize>())
}

fn half(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn span(bytes: &[u8], at: usize, length: usize) -> ReadResult<RomSpan> {
    if length == 0
        || at
            .checked_add(length)
            .is_none_or(|end| end > bytes.len() || end > MAX_ROM_BYTES)
    {
        return Err(ReadError::Invalid);
    }
    Ok(RomSpan::new(at, length))
}

fn pointer(bytes: &[u8], at: usize, length: usize, align: usize) -> ReadResult<usize> {
    rom_pointer(
        bytes,
        word(bytes, at).ok_or(ReadError::Invalid)?,
        length,
        align,
    )
    .ok_or(ReadError::Invalid)
}
