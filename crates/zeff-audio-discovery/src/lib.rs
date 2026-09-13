#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

pub mod aas;
pub mod aas_pcm;
pub mod aas_stream;
pub mod camelot;
pub mod catalog;
#[cfg(not(target_arch = "wasm32"))]
pub mod cdda;
pub mod classification;
pub mod descriptor_midi;
pub mod detectors;
pub mod drivers;
pub mod engine_software;
pub mod formats;
pub mod gax;
pub mod gax_native;
pub mod gb_carillon;
pub mod gb_ghx;
pub mod gb_music;
pub mod gb_musyx;
pub mod gb_native;
pub mod gb_quickthunder;
pub mod gb_sound_system;
pub mod gb_tose;
pub mod gba_bootstrap;
pub mod gbass;
pub mod krawall;
pub mod mp2k;
pub mod musyx;
pub mod native_rips;
pub mod natsume;
pub mod nes_music;
pub mod nes_native;
pub mod nes_tose;
pub mod nsq;
pub mod radriver;
pub mod relations;
pub mod rips;
pub mod sample;
mod scan;
pub mod sega_psg;
mod standalone;
pub mod tables;
pub mod tracker;
pub mod vgm;
pub mod ws_tose;

pub use report::{
    DetectorOutcome, DetectorState, MalformedInput, MediaIdentity, ScanLimits, ScanReport,
    ScanStatus, ScanStop,
};
pub use sample::SampleDirection;
pub use scan::scan;
pub use standalone::scan_standalone_tracker;
pub use tables::SongTableEntryKind;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;

pub const MAX_ROM_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SCAN_WORK: u64 = 100_000_000;
pub const MAX_CANDIDATES: u32 = 4096;

mod report;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct RomSpan {
    pub effective_offset: u32,
    pub byte_len: u32,
    pub canonical_cpu_address: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub effective_offset: u32,
    pub byte_len: u32,
    pub canonical_cpu_address: Option<u32>,
}

impl From<RomSpan> for SourceSpan {
    fn from(span: RomSpan) -> Self {
        Self {
            effective_offset: span.effective_offset,
            byte_len: span.byte_len,
            canonical_cpu_address: Some(span.canonical_cpu_address),
        }
    }
}

impl From<tracker::FileSpan> for SourceSpan {
    fn from(span: tracker::FileSpan) -> Self {
        Self {
            effective_offset: span.offset,
            byte_len: span.byte_len,
            canonical_cpu_address: None,
        }
    }
}

impl RomSpan {
    fn new(offset: usize, len: usize) -> Self {
        debug_assert!(offset <= MAX_ROM_BYTES && len <= MAX_ROM_BYTES - offset);
        Self {
            effective_offset: offset as u32,
            byte_len: len as u32,
            canonical_cpu_address: 0x0800_0000 + offset as u32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SongCandidate {
    pub engine: EngineProfile,
    pub header: RomSpan,
    pub priority: u8,
    pub reverb: u8,
    pub voicegroup_address: u32,
    pub voicegroup_offset: u32,
    pub confidence: Confidence,
    pub tracks: Vec<TrackInventory>,
    pub instruments: Vec<InstrumentInventory>,
    pub evidence: CandidateEvidence,
    pub warnings: Vec<Warning>,
    pub table_entries: Vec<SongTableReference>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineProfile {
    #[default]
    Mp2k,
    Mp2kSongId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SongTableReference {
    pub table_offset: u32,
    pub index: u32,
    pub entry: RomSpan,
    pub player: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CandidateEvidence {
    pub engine_signature_verified: bool,
    pub song_table_verified: bool,
    pub decoded_tracks: u8,
    pub validated_instruments: u16,
    pub explicit_note_data: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Structural,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TrackInventory {
    pub entry_address: u32,
    pub spans: Vec<RomSpan>,
    pub event_count: u32,
    pub note_count: u32,
    pub voices: Vec<u8>,
    pub voice_keys: Vec<VoiceKeys>,
    pub termination: TrackTermination,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VoiceKeys {
    pub voice: u8,
    pub keys: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackTermination {
    Fine,
    RuntimeSongReturn,
    Loop,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstrumentInventory {
    pub voice: u8,
    #[serde(flatten)]
    pub tone: ToneInventory,
    pub key_map: Option<RomSpan>,
    pub regions: Vec<InstrumentRegion>,
}

impl std::ops::Deref for InstrumentInventory {
    type Target = ToneInventory;

    fn deref(&self) -> &Self::Target {
        &self.tone
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstrumentRegion {
    pub key_start: u8,
    pub key_end: u8,
    pub descriptor_index: u8,
    pub tone: Option<ToneInventory>,
    pub warning: Option<Warning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToneInventory {
    pub descriptor: RomSpan,
    pub kind: u8,
    pub data_word: u32,
    pub key: u8,
    pub length: u8,
    pub pan_sweep: u8,
    pub adsr: Option<[u8; 4]>,
    pub fixed_pitch: bool,
    pub sample_header: Option<RomSpan>,
    pub sample: Option<SampleInventory>,
    pub waveform: Option<RomSpan>,
    pub synthesis: Option<camelot::SynthRecipe>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SampleInventory {
    pub header: RomSpan,
    pub data: RomSpan,
    pub frequency: u32,
    pub decoded_len: u32,
    pub encoding: sample::SampleEncoding,
    pub direction: sample::SampleDirection,
    pub loop_start: u32,
    pub looped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Warning {
    UnsupportedCommand {
        offset: u32,
        opcode: u8,
    },
    InvalidTrack {
        offset: u32,
    },
    TrackLimit {
        offset: u32,
    },
    UnsupportedInstrument {
        voice: u8,
        kind: u8,
    },
    InvalidInstrument {
        voice: u8,
    },
    EmptySample {
        voice: u8,
        offset: u32,
    },
    NoVoiceSelection,
    NoExplicitNoteData,
    UnsupportedTrackCount {
        count: u8,
    },
    UnresolvedInstrumentKeys {
        voice: u8,
    },
    InvalidInstrumentRegion {
        voice: u8,
        key: u8,
        offset: u32,
    },
    UnsupportedInstrumentRegion {
        voice: u8,
        key: u8,
        kind: u8,
        offset: u32,
    },
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    remaining: u64,
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn rom_pointer(bytes: &[u8], address: u32, len: usize, align: usize) -> Option<usize> {
    if !(0x0800_0000..0x0A00_0000).contains(&address) {
        return None;
    }
    let offset = (address - 0x0800_0000) as usize;
    (offset.is_multiple_of(align) && bytes.get(offset..offset.checked_add(len)?).is_some())
        .then_some(offset)
}

impl Budget<'_> {
    fn charge(&mut self) -> Result<(), ScanStop> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ScanStop::Cancelled);
        }
        if self.remaining == 0 {
            return Err(ScanStop::WorkLimit);
        }
        self.remaining -= 1;
        Ok(())
    }
}

#[cfg(feature = "fuzzing")]
pub mod fuzzing;
