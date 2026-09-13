use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::{Budget, RomSpan, ScanStop};

#[cfg(any(feature = "fuzzing", test))]
mod fuzzing;
mod sequence;
#[cfg(feature = "fuzzing")]
pub(crate) use fuzzing::fuzz_parse;
#[cfg(test)]
mod tests;

pub const PROFILE: &str = "gba-natsume-driver-v1-rev0";
const MAX_EVENTS: usize = 32_768;
const MAX_VALIDATION_WORK: u64 = 4_000_000;
const CHANNEL_KINDS: [u8; 12] = [0, 1, 2, 3, 0, 1, 2, 3, 4, 5, 4, 5];

struct Profile {
    name: &'static str,
    sha256: &'static str,
    table: usize,
    count: u16,
    channel_config: usize,
    silence: Option<u16>,
    witnesses: &'static [(usize, usize)],
}

const PROFILES: [Profile; 2] = [
    Profile {
        name: PROFILE,
        sha256: "c1d194d9ffc703066109563aef173520d8c1f2f5bbb4fd4a7b7ebb9ff98c31cc",
        table: 0x99718,
        count: 32,
        channel_config: 0x315258,
        silence: None,
        witnesses: &[(0x62c, 0x44), (0x54d10, 0x1a0), (0x52d04, 0x810)],
    },
    Profile {
        name: "gba-natsume-driver-v1-rev1",
        sha256: "78c3b7f9ac9dbc4380237d08743a82fb04c92fb0a67c3d1803e7df5e2cea92c5",
        table: 0xaff94,
        count: 54,
        channel_config: 0x3abbc0,
        silence: Some(53),
        witnesses: &[(0x58c, 0x44), (0x74870, 0x184), (0x726f0, 0x810)],
    },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NatsumeSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub kind: NatsumeSongKind,
    pub table_entry: RomSpan,
    pub header: RomSpan,
    pub channel_mask: u16,
    pub priority: u8,
    pub channels: Vec<NatsumeChannel>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NatsumeSongKind {
    Music,
    Setup,
    Silence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NatsumeChannel {
    pub number: u8,
    pub hardware_kind: u8,
    pub entry: RomSpan,
    pub event_count: u32,
    pub note_count: u32,
    pub wait_units: u32,
    pub termination: NatsumeTermination,
    pub loop_start_wait_units: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NatsumeTermination {
    Fine,
    Loop,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NatsumeSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    scan_profile(bytes, songs, budget, remaining, profile)
}

fn scan_profile(
    bytes: &[u8],
    songs: &mut Vec<NatsumeSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
    profile: &Profile,
) -> Result<(), ScanStop> {
    for index in 0..profile.count {
        budget.charge()?;
        if usize::from(index) >= remaining {
            return Err(ScanStop::CandidateLimit);
        }
        match inspect(bytes, index, profile, budget) {
            Ok(song) => songs.push(song),
            Err(sequence::ReadError::Stop(stop)) => return Err(stop),
            Err(sequence::ReadError::Invalid(_)) => return Err(ScanStop::ValidationLimit),
        }
    }
    Ok(())
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if bytes.len() != 8 * 1024 * 1024 {
        return Ok(None);
    }
    let sha256 = const_hex::encode(zeff_firmware::sha256_bytes(bytes));
    budget.charge()?;
    Ok(PROFILES.iter().find(|profile| profile.sha256 == sha256))
}

fn inspect(
    bytes: &[u8],
    index: u16,
    profile: &Profile,
    budget: &mut Budget<'_>,
) -> Result<NatsumeSong, sequence::ReadError> {
    use sequence::{ReadError, range};

    if index >= profile.count {
        return Err(ReadError::Invalid("invalid song index"));
    }
    let entry = profile.table + usize::from(index) * 4;
    let address = u32::from_le_bytes(range(bytes, entry, 4)?.try_into().unwrap());
    let header = pointer(bytes, address, 4, 4)?;
    let channel_mask = u16::from_le_bytes(range(bytes, header, 2)?.try_into().unwrap());
    if channel_mask == 0 || channel_mask & 0xf000 != 0 || bytes[header + 3] != 0 {
        return Err(ReadError::Invalid(
            "invalid little-endian channel mask or header padding",
        ));
    }
    let header_len = 4 + channel_mask.count_ones() as usize * 4;
    range(bytes, header, header_len)?;
    range(bytes, profile.channel_config, 13 * 12)?;
    let kind = if index <= 1 {
        NatsumeSongKind::Setup
    } else if profile.silence == Some(index) {
        NatsumeSongKind::Silence
    } else {
        NatsumeSongKind::Music
    };
    let title = match kind {
        NatsumeSongKind::Setup => "Stop / setup".into(),
        NatsumeSongKind::Silence => "Silence".into(),
        NatsumeSongKind::Music => format!("Natsume song {index}"),
    };
    let mut song = NatsumeSong {
        profile: profile.name,
        index,
        title,
        kind,
        table_entry: RomSpan::new(entry, 4),
        header: RomSpan::new(header, header_len),
        channel_mask,
        priority: bytes[header + 2],
        channels: Vec::new(),
        mapped_spans: vec![
            RomSpan::new(entry, 4),
            RomSpan::new(header, header_len),
            RomSpan::new(profile.channel_config, 13 * 12),
        ],
        warnings: vec![
            "Wait units and repeated control states describe structural sequence traversal; playback timing and audible loop lengths are not qualified.".into(),
            "Mapped bytes include the selected table entry, header, visited sequence commands and driver witnesses; the instrument, sample and percussion graph is not mapped.".into(),
            "MIDI and instrument-bank export are unavailable for this profile.".into(),
        ],
    };
    for &(offset, len) in profile.witnesses {
        budget.charge()?;
        range(bytes, offset, len)?;
        song.mapped_spans.push(RomSpan::new(offset, len));
    }
    let mut pointer_offset = header + 4;
    for (number, expected_kind) in CHANNEL_KINDS.iter().copied().enumerate() {
        budget.charge()?;
        let hardware_kind = bytes[profile.channel_config + number * 12 + 8];
        if hardware_kind != expected_kind {
            return Err(ReadError::Invalid(
                "channel configuration differs from the driver profile",
            ));
        }
        // The driver starts at bit 11 and advances pointers only for enabled channels.
        if channel_mask & (0x800 >> number) == 0 {
            continue;
        }
        let address = u32::from_le_bytes(range(bytes, pointer_offset, 4)?.try_into().unwrap());
        pointer_offset += 4;
        let start = pointer(bytes, address, 1, 1)?;
        let (channel, spans) =
            sequence::inspect(bytes, number as u8, hardware_kind, start, budget)?;
        song.channels.push(channel);
        song.mapped_spans.extend(spans);
    }
    sequence::merge_spans(&mut song.mapped_spans);
    Ok(song)
}

fn pointer(
    bytes: &[u8],
    address: u32,
    len: usize,
    align: usize,
) -> Result<usize, sequence::ReadError> {
    super::rom_pointer(bytes, address, len, align).ok_or(sequence::ReadError::Invalid(
        "sequence pointer leaves canonical ROM data",
    ))
}

pub fn validate_song(bytes: &[u8], song: &NatsumeSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(sequence::error)?
        .ok_or_else(|| anyhow::anyhow!("unrecognized Natsume music profile"))?;
    validate_profile_song(bytes, song, profile, &mut budget)
}

fn validate_profile_song(
    bytes: &[u8],
    song: &NatsumeSong,
    profile: &Profile,
    budget: &mut Budget<'_>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        profile.name == song.profile,
        "Natsume profile differs from its source"
    );
    let checked = inspect(bytes, song.index, profile, budget).map_err(sequence::error)?;
    anyhow::ensure!(
        checked == *song,
        "Natsume music inventory differs from its source"
    );
    Ok(())
}
