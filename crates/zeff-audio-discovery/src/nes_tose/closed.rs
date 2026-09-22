use std::sync::atomic::AtomicBool;

use crate::{Budget, RomSpan, ScanStop, nes_native::NesNativeTiming};

use super::{NesToseSong, NesToseTrack, closed_profiles::PROFILES};

pub(super) enum Input {
    Accumulator,
    Y,
    Memory(u16),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrgMapping {
    Standard,
    Mmc1Upper16K,
    Mmc3A0008K,
}

pub(super) struct Profile {
    pub id: &'static str,
    pub header: [u8; 16],
    pub byte_len: usize,
    pub prg_hash: &'static str,
    pub mapper: u16,
    pub timing: NesNativeTiming,
    pub mapping: PrgMapping,
    pub bank: u8,
    pub init: u16,
    pub selector: u16,
    pub tick: u16,
    pub input: Input,
    pub frame_control: u8,
    pub code: (u16, u16),
    pub table: u16,
    pub groups: &'static [Group],
}

pub(super) struct Group {
    pub index: u8,
    pub start: u16,
    pub end: u16,
    pub notes: [u32; 4],
}

impl Group {
    pub const fn new(index: u8, start: u16, end: u16, notes: [u32; 4]) -> Self {
        Self {
            index,
            start,
            end,
            notes,
        }
    }
}

impl Profile {
    pub fn span(&self, address: u16, len: u16) -> RomSpan {
        let offset = if self.mapping == PrgMapping::Mmc1Upper16K {
            if address < 0xc000 {
                u32::from(address - 0x8000)
            } else {
                u32::from(self.bank) * 0x4000 + u32::from(address - 0xc000)
            }
        } else if self.mapping == PrgMapping::Mmc3A0008K {
            match address {
                0x8000..=0x9fff => u32::from(address - 0x8000),
                0xa000..=0xbfff => u32::from(self.bank) * 0x2000 + u32::from(address - 0xa000),
                _ => (u32::from(self.header[4]) - 1) * 0x4000 + u32::from(address - 0xc000),
            }
        } else if self.mapper == 3 {
            u32::from(address - 0x8000)
        } else if address < 0xc000 {
            u32::from(self.bank) * 0x4000 + u32::from(address - 0x8000)
        } else {
            (u32::from(self.header[4]) - 1) * 0x4000 + u32::from(address - 0xc000)
        };
        RomSpan {
            effective_offset: 16 + offset,
            byte_len: u32::from(len),
            canonical_cpu_address: u32::from(address),
        }
    }

    fn recognized(&self, bytes: &[u8], budget: &mut Budget<'_>) -> Result<bool, ScanStop> {
        budget.charge()?;
        if bytes.len() != self.byte_len || bytes.get(..16) != Some(self.header.as_slice()) {
            return Ok(false);
        }
        let end = 16 + usize::from(self.header[4]) * 0x4000;
        for _ in 0..(end - 16).div_ceil(256) {
            budget.charge()?;
        }
        // Fixed groups and their sequence closure require the exact qualified PRG.
        let hash = zeff_firmware::sha256_hex(&bytes[16..end]);
        if hash == self.prg_hash {
            return Ok(true);
        }
        #[cfg(any(test, feature = "test-support"))]
        if super::closed_tests::fixture_hash(self, &hash) {
            return Ok(true);
        }
        Ok(false)
    }

    fn song(&self, bytes: &[u8], group: &Group) -> NesToseSong {
        let address = self.table + u16::from(group.index) * 4;
        let table_entry = self.span(address, 16);
        let offset = table_entry.effective_offset as usize;
        let tracks = (0..4)
            .map(|channel| NesToseTrack {
                number: bytes[offset + channel * 4 + 1] + 1,
                note_count: group.notes[channel],
            })
            .collect();
        // Branches and RTS also read the page prefix and following byte.
        let mut mapped_spans = vec![
            self.span(
                self.code.0 & 0xff00,
                self.code.1 - (self.code.0 & 0xff00) + 1,
            ),
            self.span(address & 0xff00, (address & 255) + 16),
            self.span(group.start, group.end - group.start),
        ];
        mapped_spans.sort_unstable();
        let mut merged: Vec<RomSpan> = Vec::new();
        for span in mapped_spans {
            if let Some(last) = merged.last_mut()
                && last.effective_offset + last.byte_len >= span.effective_offset
            {
                last.byte_len = last
                    .byte_len
                    .max(span.effective_offset + span.byte_len - last.effective_offset);
            } else {
                merged.push(span);
            }
        }
        NesToseSong {
            profile: self.id,
            index: u16::from(group.index),
            title: format!("Native selector group {}", group.index),
            table_entry,
            tracks,
            mapped_spans: merged,
            warnings: vec!["Fixed source profile with qualified four-channel groups; roles, duration and full soundtrack coverage are not established.".into()],
        }
    }
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for profile in PROFILES {
        if !profile.recognized(bytes, budget)? {
            continue;
        }
        for group in profile.groups {
            budget.charge()?;
            if songs.len() >= remaining {
                return Err(ScanStop::CandidateLimit);
            }
            songs.push(profile.song(bytes, group));
        }
        break;
    }
    Ok(())
}

pub(super) fn profile(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|profile| profile.id == id)
}

pub(super) fn validate_song(
    bytes: &[u8],
    expected: &NesToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let valid = if let Some(profile) = profile(expected.profile) {
        profile
            .recognized(bytes, &mut budget)
            .map_err(|stop| anyhow::anyhow!("NES TOSE validation stopped: {stop:?}"))?
            && profile.groups.iter().any(|group| {
                u16::from(group.index) == expected.index && profile.song(bytes, group) == *expected
            })
    } else {
        false
    };
    anyhow::ensure!(
        valid,
        "NES TOSE selection differs from its recognized source"
    );
    Ok(())
}
