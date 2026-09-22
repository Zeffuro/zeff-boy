use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};

use super::{NesNativeProfile, NesNativeSong, NesNativeTiming, PreparedNesNative};
use crate::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_rom, fixture_rom_cnrom};

const NROM_PROFILE: &str = "nes-native-register-presets-nrom-01";
const CNROM_PROFILE: &str = "nes-native-register-presets-cnrom-01";
const SOURCE_LEN: usize = 0xa010;
const NROM_HASH: &str = "49a5dd402f0566cf052f8ea1ebbec34bb83858453746d518aeb4f2fd6d514fab";
const CNROM_HASH: &str = "9f3d4d500c1b1e0bd931a71fba2ad1450433fd30fbdbad8e80f57c2019e4063d";
const NROM_HEADER: &[u8; 16] = &[0x4e, 0x45, 0x53, 0x1a, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const CNROM_HEADER: &[u8; 16] = &[
    0x4e, 0x45, 0x53, 0x1a, 2, 1, 0x31, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const RAW: &[u8] = &[0, 1, 2, 3, 5, 6, 8];
const TABLE: u16 = 0xd956;
const INIT: u16 = 0xe9cb;
const TICK: u16 = 0xe9ff;
const BOOTSTRAP: u16 = 0xf800;
const READY: u16 = 0x07f0;
const ACK: u16 = 0x07f1;

#[derive(Clone, Copy)]
struct Profile {
    id: &'static str,
    mapper: u16,
    header: &'static [u8; 16],
    hash: &'static str,
}

const PROFILES: &[Profile] = &[
    Profile {
        id: NROM_PROFILE,
        mapper: 0,
        header: NROM_HEADER,
        hash: NROM_HASH,
    },
    Profile {
        id: CNROM_PROFILE,
        mapper: 3,
        header: CNROM_HEADER,
        hash: CNROM_HASH,
    },
];

pub(super) fn owns(profile: &str) -> bool {
    PROFILES.iter().any(|entry| entry.id == profile) || fixture_profile(profile)
}

fn fixture_profile(profile: &str) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if profile == fixture::PROFILE_NROM || profile == fixture::PROFILE_CNROM {
        return true;
    }
    let _ = profile;
    false
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Profile>, ScanStop> {
    if bytes.len() != SOURCE_LEN {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    if let Some(profile) = PROFILES
        .iter()
        .copied()
        .find(|profile| bytes.get(..16) == Some(profile.header) && hash == profile.hash)
    {
        return Ok(Some(profile));
    }
    #[cfg(any(test, feature = "test-support"))]
    if let Some(profile) = fixture::profile(bytes, &hash) {
        return Ok(Some(profile));
    }
    Ok(None)
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for (index, &raw_index) in RAW.iter().enumerate() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(inspect(bytes, profile, index, raw_index).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
}

pub(super) fn prepare(
    bytes: &[u8],
    song: &NesNativeSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedNesNative> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES preset validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("NES source no longer matches its preset profile"))?;
    let index = usize::from(song.index);
    ensure!(
        RAW.get(index)
            .and_then(|&raw| inspect(bytes, profile, index, raw))
            .as_ref()
            == Some(song),
        "NES preset selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "NES preset preparation cancelled"
    );
    build(bytes, song)
}

fn inspect(bytes: &[u8], profile: Profile, index: usize, raw_index: u8) -> Option<NesNativeSong> {
    let table_entry = span(bytes, TABLE.checked_add(u16::from(raw_index) * 4)?, 4)?;
    let record = bytes.get(table_entry.effective_offset as usize..)?;
    let native = NesNativeProfile {
        mapper: profile.mapper,
        timing: NesNativeTiming::Ntsc,
        init: span(bytes, INIT, 0xd5)?,
        tick: span(bytes, TICK, 0xa1)?,
        driver: span(bytes, INIT, 0xd5)?,
        tables: span(bytes, TABLE, 0x24)?,
        bootstrap: span(bytes, BOOTSTRAP, 0x100)?,
    };
    let mapped_spans = vec![span(bytes, TABLE, 0x24)?, span(bytes, 0xe900, 0x1a0)?];
    if record.len() < 4
        || mapped_spans
            .iter()
            .any(|span| intersects(*span, native.bootstrap))
    {
        return None;
    }
    Some(NesNativeSong {
        profile: profile.id,
        index: index as u16,
        raw_index,
        title: format!("APU preset {raw_index}"),
        header: table_entry,
        table_entry,
        channels: Vec::new(),
        native,
        mapped_spans,
        warnings: vec![
            "This is an isolated NTSC-frame projection of a caller-proven APU preset; presets may sustain for the requested duration.".into(),
            "These hardware presets are not sequenced music. Music role, game sequencing and complete soundtrack coverage are not established.".into(),
        ],
    })
}

pub(super) fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    media.system == "nes"
        && media.byte_len == SOURCE_LEN as u64
        && media.sha256.as_deref().is_some_and(identity_matches)
        && span.byte_len != 0
        && span.effective_offset >= 16
        && u64::from(span.effective_offset) + u64::from(span.byte_len) <= 0x8010
        && span.canonical_cpu_address == Some(0x8000 + span.effective_offset - 16)
}

fn identity_matches(hash: &str) -> bool {
    PROFILES.iter().any(|profile| profile.hash == hash) || fixture_identity_matches(hash)
}

fn fixture_identity_matches(hash: &str) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if hash == fixture::source_hash(false) || hash == fixture::source_hash(true) {
        return true;
    }
    let _ = hash;
    false
}

fn span(bytes: &[u8], address: u16, len: usize) -> Option<RomSpan> {
    super::prg_span(bytes, address, len)
}

fn intersects(a: RomSpan, b: RomSpan) -> bool {
    a.effective_offset < b.effective_offset + b.byte_len
        && b.effective_offset < a.effective_offset + a.byte_len
}

fn build(bytes: &[u8], song: &NesNativeSong) -> AnyResult<PreparedNesNative> {
    let cpu = song.native.bootstrap.canonical_cpu_address as u16;
    let patch = song.native.bootstrap;
    let mut code = vec![0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0];
    for page in 0..8 {
        code.extend_from_slice(&[0x9d, 0, page]);
    }
    code.extend_from_slice(&[0xe8, 0xd0, 0xe5]);
    code.extend_from_slice(&[0x8d, 0, 0x20, 0x8d, 1, 0x20]);
    code.extend_from_slice(&[0xa9, 0x0f, 0x8d, 0x15, 0x40]);
    code.extend_from_slice(&[0xa9, 0x40, 0x8d, 0x17, 0x40]);
    code.extend_from_slice(&[0x20, INIT as u8, (INIT >> 8) as u8]);
    code.extend_from_slice(&[0xa9, 0xff, 0x85, 0xad, 0xa9, 1, 0x8d]);
    code.extend_from_slice(&READY.to_le_bytes());
    let wait = code.len();
    code.extend_from_slice(&[0xad, ACK as u8, (ACK >> 8) as u8, 0xc9, 1, 0xd0, 0xf9]);
    code.extend_from_slice(&[0x2c, 2, 0x20]);
    code.extend_from_slice(&[
        0xa9,
        song.raw_index,
        0x85,
        0xad,
        0xa9,
        0x80,
        0x8d,
        0,
        0x20,
        0x58,
    ]);
    let idle = cpu
        .checked_add(u16::try_from(code.len())?)
        .ok_or_else(|| anyhow::anyhow!("NES preset bootstrap address overflow"))?;
    code.extend_from_slice(&[0x4c, idle as u8, (idle >> 8) as u8]);
    let nmi = cpu
        .checked_add(u16::try_from(code.len())?)
        .ok_or_else(|| anyhow::anyhow!("NES preset NMI address overflow"))?;
    code.extend_from_slice(&[
        0x48,
        0x8a,
        0x48,
        0x98,
        0x48,
        0x20,
        TICK as u8,
        (TICK >> 8) as u8,
    ]);
    code.extend_from_slice(&[0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40]);
    ensure!(
        code.len() <= patch.byte_len as usize && u32::from(cpu) + patch.byte_len <= 0xf900,
        "NES preset bootstrap exceeds its qualified patch window"
    );
    let mut result = bytes.to_vec();
    let offset = patch.effective_offset as usize;
    result
        .get_mut(offset..offset + code.len())
        .ok_or_else(|| anyhow::anyhow!("NES preset bootstrap is outside its source"))?
        .copy_from_slice(&code);
    let vectors = result
        .get_mut(0x800a..0x8010)
        .ok_or_else(|| anyhow::anyhow!("NES preset vectors are missing"))?;
    vectors[..2].copy_from_slice(&nmi.to_le_bytes());
    vectors[2..4].copy_from_slice(&cpu.to_le_bytes());
    vectors[4..].copy_from_slice(&(nmi + 13).to_le_bytes());
    Ok(PreparedNesNative {
        bytes: result,
        mapper: song.native.mapper,
        timing: NesNativeTiming::Ntsc,
        ready_address: READY,
        ack_address: ACK,
        wait_start: cpu + wait as u16,
        wait_end: cpu + wait as u16 + 7,
    })
}
