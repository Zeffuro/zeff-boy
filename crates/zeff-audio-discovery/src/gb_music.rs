use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::{Budget, ScanStop, tracker::FileSpan};

pub mod midi;
pub mod native;
mod sequence;

pub use midi::midi;
#[cfg(test)]
mod tests;

pub(crate) const PROFILE: &str = "gb-banked-driver-v1-rev0";
const REV1_PROFILE: &str = "gb-banked-driver-v1-rev1";
const REV2_PROFILE: &str = "gb-banked-driver-v1-rev2";
const REV3_PROFILE: &str = "gb-banked-driver-v1-rev3";
const REV4_PROFILE: &str = "gb-banked-driver-v1-rev4";
const REV5_PROFILE: &str = "gb-banked-driver-v1-rev5";
const MAX_EVENTS: usize = 32_768;
const MAX_FRAMES: u32 = 430_038;

struct Profile {
    name: &'static str,
    sha256: &'static str,
    table: usize,
    song_count: usize,
}

static PROFILES: [Profile; 6] = [
    Profile {
        name: PROFILE,
        sha256: "d6702e353dcbe2d2c69183046c878ef13a0dae4006e8cdff521cca83dd1582fe",
        table: 0xe906e,
        song_count: 103,
    },
    Profile {
        name: REV1_PROFILE,
        sha256: "fdcc3c8c43813cf8731fc037d2a6d191bac75439c34b24ba1c27526e6acdc8a2",
        table: 0xe906e,
        song_count: 103,
    },
    Profile {
        name: REV2_PROFILE,
        sha256: "136ada06cb68656b7de475fa4b278d37dbeff8f5257e7dfdf7f4a4aec19a90f3",
        table: 0xe906e,
        song_count: 103,
    },
    Profile {
        name: REV3_PROFILE,
        sha256: "fb0016d27b1e5374e1ec9fcad60e6628d8646103b5313ca683417f52b97e7e4e",
        table: 0xe906e,
        song_count: 93,
    },
    Profile {
        name: REV4_PROFILE,
        sha256: "72b190859a59623cbef6c49d601f8de52c1d2331b4f08a8d2acc17274fc19a8c",
        table: 0xe906e,
        song_count: 93,
    },
    Profile {
        name: REV5_PROFILE,
        sha256: "d280718ad34af96035b562922a8893d790ea2548e9014a035d5f4befd3fd1edf",
        table: 0xe906e,
        song_count: 93,
    },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub table_entry: FileSpan,
    pub header: FileSpan,
    pub bank: u8,
    pub cpu_address: u16,
    pub channels: Vec<GbChannel>,
    pub mapped_spans: Vec<FileSpan>,
    pub warnings: Vec<GbWarning>,
    pub midi_exportable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbChannel {
    pub number: u8,
    pub entry: FileSpan,
    pub cpu_address: u16,
    pub event_count: u32,
    pub note_count: u32,
    pub end_frame: u32,
    pub termination: GbTermination,
    pub loop_start_frame: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbTermination {
    Fine,
    Loop,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbWarning {
    pub offset: u32,
    pub reason: String,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for index in 0..profile.song_count {
        budget.charge()?;
        if index >= remaining {
            return Err(ScanStop::CandidateLimit);
        }
        match inspect(bytes, index, profile, budget) {
            Ok(program) => songs.push(program.song),
            Err(sequence::ReadError::Stop(stop)) => return Err(stop),
            Err(sequence::ReadError::Invalid(_)) => return Err(ScanStop::ValidationLimit),
        }
    }
    Ok(())
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if bytes.len() != 2 * 1024 * 1024 {
        return Ok(None);
    }
    let sha256 = const_hex::encode(zeff_firmware::sha256_bytes(bytes));
    let profile = profile_by_sha(&sha256);
    budget.charge()?;
    Ok(profile)
}

fn profile_by_sha(sha256: &str) -> Option<&'static Profile> {
    let profile = PROFILES.iter().find(|profile| profile.sha256 == sha256);
    #[cfg(any(test, feature = "test-support"))]
    let profile = profile.or_else(|| native::fixture_profile(sha256));
    profile
}

fn inspect(
    bytes: &[u8],
    index: usize,
    profile: &'static Profile,
    budget: &mut Budget<'_>,
) -> Result<sequence::Program, sequence::ReadError> {
    let entry = profile.table + index * 3;
    let table = bytes
        .get(entry..entry + 3)
        .ok_or_else(|| sequence::ReadError::Invalid("music table is truncated".into()))?;
    let bank = table[0];
    let address = u16::from_le_bytes([table[1], table[2]]);
    let mut song = header(bytes, index as u16, entry, bank, address)?;
    song.profile = profile.name;
    sequence::interpret(bytes, song, 1, MAX_FRAMES, budget)
}

fn header(
    bytes: &[u8],
    index: u16,
    table: usize,
    bank: u8,
    address: u16,
) -> Result<GbSong, sequence::ReadError> {
    let offset = pointer(bytes, bank, address, 1)?;
    let count = usize::from(bytes[offset] >> 6) + 1;
    let offset = pointer(bytes, bank, address, count * 3)?;
    let mut channels = Vec::new();
    for channel in bytes[offset..offset + count * 3].as_chunks::<3>().0 {
        let number = (channel[0] & 7) + 1;
        if number > 4
            || channel[0] & 0x38 != 0
            || (!channels.is_empty() && channel[0] & 0xc0 != 0)
            || channels
                .iter()
                .any(|other: &GbChannel| other.number == number)
        {
            return Err(sequence::ReadError::Invalid(
                "invalid music channel header".into(),
            ));
        }
        let cpu_address = u16::from_le_bytes([channel[1], channel[2]]);
        let entry = pointer(bytes, bank, cpu_address, 1)?;
        channels.push(GbChannel {
            number,
            entry: span(entry, 1),
            cpu_address,
            event_count: 0,
            note_count: 0,
            end_frame: 0,
            termination: GbTermination::Unresolved,
            loop_start_frame: None,
        });
    }
    channels.sort_by_key(|channel| channel.number);
    Ok(GbSong {
        profile: PROFILE,
        index,
        title: if index == 0 {
            "Nothing (silence)".into()
        } else {
            format!("GB song {index}")
        },
        table_entry: span(table, 3),
        header: span(offset, count * 3),
        bank,
        cpu_address: address,
        channels,
        mapped_spans: Vec::new(),
        warnings: Vec::new(),
        midi_exportable: false,
    })
}

fn pointer(bytes: &[u8], bank: u8, address: u16, len: usize) -> Result<usize, sequence::ReadError> {
    if bank == 0 || !(0x4000..0x8000).contains(&address) || usize::from(address) + len > 0x8000 {
        return Err(sequence::ReadError::Invalid(
            "music pointer leaves its ROM bank".into(),
        ));
    }
    let offset = usize::from(bank) * 0x4000 + usize::from(address) - 0x4000;
    if offset.checked_add(len).is_none_or(|end| end > bytes.len()) {
        return Err(sequence::ReadError::Invalid(
            "music pointer leaves the source".into(),
        ));
    }
    Ok(offset)
}

fn span(offset: usize, byte_len: usize) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len: byte_len as u32,
    }
}

pub fn validate_song(bytes: &[u8], song: &GbSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(sequence::error)?
        .ok_or_else(|| anyhow::anyhow!("unrecognized Game Boy music profile"))?;
    anyhow::ensure!(
        usize::from(song.index) < profile.song_count,
        "invalid Game Boy song index"
    );
    let checked =
        inspect(bytes, usize::from(song.index), profile, &mut budget).map_err(sequence::error)?;
    anyhow::ensure!(
        checked.song == *song,
        "Game Boy music inventory differs from its source"
    );
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn synthetic_song_for_test_support(bytes: &[u8], bank: u8, address: u16) -> GbSong {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let song = header(bytes, 1, 0x100, bank, address).expect("synthetic song header");
    sequence::interpret(bytes, song, 1, MAX_FRAMES, &mut budget)
        .expect("synthetic song sequence")
        .song
}
