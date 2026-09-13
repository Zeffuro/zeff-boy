use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::{Budget, ScanStop, tracker::FileSpan};

pub mod midi;
mod sequence;

pub use midi::midi;
#[cfg(test)]
mod tests;

pub(crate) const PROFILE: &str = "nes-queue-driver-v1-nrom-layout";
const TABLE: u16 = 0xf90d;
const FREQUENCIES: u16 = 0xff00;
const LENGTHS: u16 = 0xff66;
const SONG_COUNT: usize = 16;
const MAX_EVENTS: usize = 65_536;
const MAX_FRAMES: u32 = 432_712;
const TITLES: [&str; SONG_COUNT] = [
    "Death",
    "Game Over",
    "Victory",
    "End of Castle",
    "Game Over (alternate selector)",
    "End of Level",
    "Time Running Out",
    "Event Silence",
    "Ground",
    "Water",
    "Underground",
    "Castle",
    "Cloud",
    "Pipe Intro",
    "Star Power",
    "Area Silence",
];

struct Witness {
    offset: usize,
    len: usize,
    sha256: &'static str,
}

const WITNESSES: [Witness; 19] = [
    Witness {
        offset: 0x791d,
        len: 8,
        sha256: "8e063c7b11b6a53a1924166f22c04ebd300407a9606d611bd494aa33817b5909",
    },
    Witness {
        offset: 0x7926,
        len: 57,
        sha256: "bbaf5ae053e873e606d2a9b79457deca0b81a7992ae2276a5d4a0d6e12582cf7",
    },
    Witness {
        offset: 0x7963,
        len: 465,
        sha256: "497fb10896def1ca7e92a56e20dde1882653bd8482e96b91b687caf537a59f8f",
    },
    Witness {
        offset: 0x7b35,
        len: 455,
        sha256: "350e592179fe36243c31cba3ee50c88680f84ce3e79994dcc919001e9f4fa355",
    },
    Witness {
        offset: 0x7cfd,
        len: 528,
        sha256: "c13998236ca918af2b4561f6338d5b9d3397dd265a14892c6822fcf6401fcb21",
    },
    Witness {
        offset: 0x7f15,
        len: 63,
        sha256: "bd5ccb4609fd6db487b37b3a0c71bf48bbe233977d2d2993c4c5905a71e36f9a",
    },
    Witness {
        offset: 0x7f56,
        len: 2,
        sha256: "d070dc5b8da9aea7dc0f5ad4c29d89965200059c9a0ceca3abd5da2492dcb71d",
    },
    Witness {
        offset: 0x7f5e,
        len: 2,
        sha256: "81904e68a8b9a2427e9e87e2c61b1098057608d18357d70d9281e8513941cf53",
    },
    Witness {
        offset: 0x7f6c,
        len: 12,
        sha256: "b810f65af6a37bf53628359af1936bc0562e769b2a39ca5bf505d13d30e2535f",
    },
    Witness {
        offset: 0x7f79,
        len: 2,
        sha256: "0e613e4615d5c0dc760e771c7c37e5527ba3c68930911bf0f8b62b1bb569fdee",
    },
    Witness {
        offset: 0x7f7e,
        len: 8,
        sha256: "6f2e54f6fea4d6c0ca9f2ef355c5c126a8201c0aebbfdf91204cf19318e1c935",
    },
    Witness {
        offset: 0x7f87,
        len: 5,
        sha256: "9afceb53934398315a704aac5f695ab33b067390ffb48e32b9150cce8cc37c04",
    },
    Witness {
        offset: 0x7f8d,
        len: 10,
        sha256: "619b72138f6cc6a5b59e2f26d1f938ef6aca3440773243f81ed5462a0b18ae8c",
    },
    Witness {
        offset: 0x7f98,
        len: 6,
        sha256: "0ce629b6749403ca9684d5bb2d9960441cfbfa9b80562f7a68e13b1196fb14e6",
    },
    Witness {
        offset: 0x790d,
        len: 16,
        sha256: "1db260ef93eef99afac7e83e6555ab52ccd70bebc14571fbda03eeaabd24139c",
    },
    Witness {
        offset: 0x7f00,
        len: 102,
        sha256: "b1df910e00a5eaddb66c8333ef4ba705b50c94f00becb00520b1cbd55b1673bd",
    },
    Witness {
        offset: 0x7f66,
        len: 48,
        sha256: "11de1686e7344c951617581e8fa4130786d261d858d68b5f5e3859a49fa99a24",
    },
    Witness {
        offset: 0x7210,
        len: 1805,
        sha256: "3bbf947cc0ad86df692742af10ddd4cc5eec451285187240bc8bfcd00317aebc",
    },
    Witness {
        offset: 0x800a,
        len: 6,
        sha256: "5926d92c3ae25ef215dd1b4697173f97e7e7064b4289958d45fb1740a4dd2085",
    },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesSong {
    pub profile: &'static str,
    pub index: u8,
    pub title: String,
    pub queue: NesQueue,
    pub selector: u8,
    pub table_entry: FileSpan,
    pub header: FileSpan,
    pub cpu_address: u16,
    pub channels: Vec<NesChannel>,
    pub sections: Vec<NesSection>,
    pub mapped_spans: Vec<FileSpan>,
    pub warnings: Vec<NesWarning>,
    pub midi_exportable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NesQueue {
    Event,
    Area,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesSection {
    pub table_entry: FileSpan,
    pub header: FileSpan,
    pub cpu_address: u16,
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesChannel {
    pub number: u8,
    pub entry: FileSpan,
    pub cpu_address: u16,
    pub event_count: u32,
    pub note_count: u32,
    pub end_frame: u32,
    pub termination: NesTermination,
    pub loop_start_frame: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NesTermination {
    Fine,
    Loop,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesWarning {
    pub offset: u32,
    pub reason: String,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    if !recognized(bytes, budget)? {
        return Ok(());
    }
    for index in 0..SONG_COUNT {
        budget.charge()?;
        if index >= remaining {
            return Err(ScanStop::CandidateLimit);
        }
        match sequence::interpret(bytes, index as u8, 1, MAX_FRAMES, budget) {
            Ok(program) => songs.push(program.song),
            Err(sequence::ReadError::Stop(stop)) => return Err(stop),
            Err(sequence::ReadError::Invalid(_)) => return Err(ScanStop::ValidationLimit),
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn synthetic_song_for_test_support(bytes: &[u8], index: u8) -> NesSong {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    sequence::interpret(bytes, index, 1, MAX_FRAMES, &mut budget)
        .expect("synthetic NES music")
        .song
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<bool, ScanStop> {
    recognized_with_witnesses(bytes, budget, &WITNESSES)
}

fn recognized_with_witnesses(
    bytes: &[u8],
    budget: &mut Budget<'_>,
    witnesses: &[Witness],
) -> Result<bool, ScanStop> {
    budget.charge()?;
    if bytes.len() != 40_976
        || bytes.get(..4) != Some(b"NES\x1a")
        || bytes.get(4) != Some(&2)
        || bytes.get(5) != Some(&1)
    {
        return Ok(false);
    }
    let flags6 = bytes[6];
    let flags7 = bytes[7];
    if flags6 & 0xfc != 0 || flags7 & 0xfc != 0 || bytes[9] & 1 != 0 {
        return Ok(false);
    }
    for witness in witnesses {
        budget.charge()?;
        let Some(region) = bytes.get(witness.offset..witness.offset + witness.len) else {
            return Ok(false);
        };
        if const_hex::encode(zeff_firmware::sha256_bytes(region)) != witness.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn pointer(bytes: &[u8], address: u16, len: usize) -> Result<usize, sequence::ReadError> {
    if address < 0x8000 || usize::from(address) + len > 0x10000 {
        return Err(sequence::ReadError::Invalid(
            "music pointer leaves NROM PRG".into(),
        ));
    }
    let offset = usize::from(address) - 0x8000 + 16;
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

pub fn validate_song(bytes: &[u8], song: &NesSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    anyhow::ensure!(
        recognized(bytes, &mut budget).map_err(sequence::error)?,
        "unrecognized NES music profile"
    );
    anyhow::ensure!(
        usize::from(song.index) < SONG_COUNT,
        "invalid NES song index"
    );
    let checked = sequence::interpret(bytes, song.index, 1, MAX_FRAMES, &mut budget)
        .map_err(sequence::error)?;
    anyhow::ensure!(
        checked.song == *song,
        "NES music inventory differs from its source"
    );
    Ok(())
}
