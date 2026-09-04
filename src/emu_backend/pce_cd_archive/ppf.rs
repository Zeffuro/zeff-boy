use std::collections::BTreeMap;
use std::sync::Arc;

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode, CdTrackSource};

use super::{PceCdArchiveCueIdentity, virtual_member_path};
use crate::emu_backend::pce_cd::{
    CONTENT_ID_DOMAIN, CueSheet, LoadedPceCd, PceCdLoadError, cue_track_layout,
};
use crate::emu_backend::pce_cd_overlay::{
    PatchOverlayBuilder, PatchOverlayStack, apply_ppf_byte_slices_stack,
};

pub(super) const ARCHIVE_PPF_PATCH_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
const ARCHIVE_PPF_STACK_BYTES_LIMIT: u64 = 128 * 1024 * 1024;
const ARCHIVE_PPF_PATCH_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceCdArchivePpfPatchIdentity {
    pub(crate) member_path: String,
    pub(crate) len: usize,
    pub(crate) sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceCdArchivePpfPatch {
    pub(crate) identity: PceCdArchivePpfPatchIdentity,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct PceCdArchivePpfLoad {
    pub(crate) cue_path: std::path::PathBuf,
    pub(crate) loaded: LoadedPceCd,
    pub(crate) archive_identity: PceCdArchiveCueIdentity,
    pub(crate) patches: Vec<PceCdArchivePpfPatch>,
    pub(crate) unpatched_disc_sha256: [u8; 32],
}

impl PceCdArchivePpfLoad {
    pub(crate) fn patch_identities(&self) -> Vec<PceCdArchivePpfPatchIdentity> {
        self.patches
            .iter()
            .map(|patch| patch.identity.clone())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceCdArchivePpfCandidate {
    pub(crate) cue_member: String,
    pub(crate) identity: PceCdArchiveCueIdentity,
    pub(crate) patches: Vec<PceCdArchivePpfPatchIdentity>,
}

#[derive(Clone, Debug)]
pub(crate) struct ArchivePpfMember {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) is_regular: bool,
}

pub(crate) struct ArchivePpfBuildInput<'a> {
    pub(crate) archive_path: &'a std::path::Path,
    pub(crate) cue_name: &'a str,
    pub(crate) cue_bytes: &'a [u8],
    pub(crate) sheet: &'a CueSheet,
    pub(crate) files: Vec<Vec<u8>>,
    pub(crate) archive_identity: PceCdArchiveCueIdentity,
    pub(crate) patches: Vec<PceCdArchivePpfPatch>,
}

pub(crate) fn discover_archive_ppf_members(
    cue_name: &str,
    members: &[ArchivePpfMember],
) -> Result<Option<Vec<String>>, PceCdLoadError> {
    let Some((cue_stem, extension)) = cue_name.rsplit_once('.') else {
        return Err(ppf_error("selected CUE member name is malformed"));
    };
    if !extension.eq_ignore_ascii_case("cue") {
        return Err(ppf_error("selected CUE member name is malformed"));
    }
    let container = format!("{cue_stem}.ppf");
    let prefix = format!("{container}/");
    let mut present = false;
    let mut slots = BTreeMap::new();
    let mut total = 0_u64;
    for member in members {
        if member.name == container {
            present = true;
            if member.is_regular {
                return Err(ppf_error("PPF container is not a directory"));
            }
            continue;
        }
        let Some(relative) = member.name.strip_prefix(&prefix) else {
            continue;
        };
        present = true;
        let Some(slot) = patch_slot(relative) else {
            return Err(ppf_error("PPF container has a malformed member"));
        };
        if !member.is_regular {
            return Err(ppf_error("PPF member is not a regular file"));
        }
        if member.size > ARCHIVE_PPF_PATCH_BYTES_LIMIT {
            return Err(ppf_error("PPF member is outside bounded limits"));
        }
        total = total
            .checked_add(member.size)
            .ok_or_else(|| ppf_error("PPF stack is outside bounded limits"))?;
        if total > ARCHIVE_PPF_STACK_BYTES_LIMIT
            || slots.insert(slot, member.name.clone()).is_some()
        {
            return Err(ppf_error("PPF stack is outside bounded limits"));
        }
    }
    if !present {
        return Ok(None);
    }
    if slots.is_empty() {
        return Err(ppf_error("PPF container is empty"));
    }
    let count = slots.len();
    if count > ARCHIVE_PPF_PATCH_LIMIT || (1..=count).any(|slot| !slots.contains_key(&slot)) {
        return Err(ppf_error("PPF stack is not contiguous"));
    }
    Ok(Some(slots.into_values().collect()))
}

pub(crate) fn patches_from_bytes(
    names: &[String],
    extracted: &mut BTreeMap<String, Vec<u8>>,
) -> Result<Vec<PceCdArchivePpfPatch>, PceCdLoadError> {
    names
        .iter()
        .map(|name| {
            let bytes = extracted
                .remove(name)
                .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.clone()))?;
            if bytes.len() as u64 > ARCHIVE_PPF_PATCH_BYTES_LIMIT {
                return Err(ppf_error("PPF member is outside bounded limits"));
            }
            Ok(PceCdArchivePpfPatch {
                identity: PceCdArchivePpfPatchIdentity {
                    member_path: name.clone(),
                    len: bytes.len(),
                    sha256: Sha256::digest(&bytes).into(),
                },
                bytes,
            })
        })
        .collect()
}

pub(crate) fn patch_identities(
    names: &[String],
    extracted: &mut BTreeMap<String, Vec<u8>>,
) -> Result<Vec<PceCdArchivePpfPatchIdentity>, PceCdLoadError> {
    patches_from_bytes(names, extracted)
        .map(|patches| patches.into_iter().map(|patch| patch.identity).collect())
}

pub(crate) fn build_archive_ppf_load(
    input: ArchivePpfBuildInput<'_>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    let ArchivePpfBuildInput {
        archive_path,
        cue_name,
        cue_bytes,
        sheet,
        files,
        archive_identity,
        patches,
    } = input;
    if patches.is_empty() {
        return Err(PceCdLoadError::NoArchivePpfStack);
    }
    let sources = files.into_iter().map(Arc::<[u8]>::from).collect::<Vec<_>>();
    let source_objects = sources
        .iter()
        .cloned()
        .map(|bytes| Arc::new(ByteSource::new(bytes)) as Arc<dyn CdTrackSource>)
        .collect::<Vec<_>>();
    let base_disc = build_disc(sheet, &source_objects)?;
    let unpatched_disc_sha256 = base_disc.content_hash();
    let mut loaded = loaded_identity(cue_bytes, sheet, &sources, base_disc)?;
    let mut builder = PatchOverlayBuilder::for_tracks(&source_objects)
        .ok_or_else(|| ppf_error("PPF source is outside bounded limits"))?;
    let patch_bytes = patches
        .iter()
        .map(|patch| (patch.identity.member_path.as_str(), patch.bytes.as_slice()))
        .collect::<Vec<_>>();
    let PatchOverlayStack::Applied(_) = apply_ppf_byte_slices_stack(&mut builder, &patch_bytes)
    else {
        return Err(ppf_error("PPF plan requires an unsupported fallback"));
    };
    let patched_sources = builder
        .finish_tracks(source_objects)
        .ok_or_else(|| ppf_error("PPF source is outside bounded limits"))?;
    loaded.disc = build_disc(sheet, &patched_sources)?;
    Ok(PceCdArchivePpfLoad {
        cue_path: virtual_member_path(archive_path, cue_name),
        loaded,
        archive_identity,
        patches,
        unpatched_disc_sha256,
    })
}

fn loaded_identity(
    cue_bytes: &[u8],
    sheet: &CueSheet,
    files: &[Arc<[u8]>],
    disc: CdDisc,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    update_identity(&mut sha, &mut crc, CONTENT_ID_DOMAIN);
    update_identity(&mut sha, &mut crc, cue_bytes);
    let count = (sheet.files.len() as u64).to_le_bytes();
    sha.update(count);
    crc.update(&count);
    for (cue_file, bytes) in sheet.files.iter().zip(files) {
        update_identity(&mut sha, &mut crc, cue_file.reference.as_bytes());
        update_identity(&mut sha, &mut crc, bytes);
    }
    let source_disc_sha256 = disc.content_hash();
    let raw_source_media_len = disc.tracks().iter().try_fold(0_usize, |total, track| {
        total.checked_add(track.sector_count() as usize * sector_bytes(track.mode()))
    });
    Ok(LoadedPceCd {
        disc,
        raw_source_media_sha256: source_disc_sha256,
        raw_source_media_len: raw_source_media_len.ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?,
        content_sha256: sha.finalize().into(),
        content_crc32: crc.finalize(),
        mod_crc32: crc32fast::hash(&source_disc_sha256),
        source_disc_sha256,
    })
}

fn build_disc(
    sheet: &CueSheet,
    sources: &[Arc<dyn CdTrackSource>],
) -> Result<CdDisc, PceCdLoadError> {
    let lengths = sources
        .iter()
        .map(|source| source.len())
        .collect::<Vec<_>>();
    let layout = cue_track_layout(sheet, &lengths)?;
    let mut tracks = Vec::with_capacity(sheet.tracks.len());
    for file_layout in layout {
        for track_layout in file_layout {
            let track = track_layout.track;
            let source = crate::emu_backend::pce_cd_overlay::slice_source(
                Arc::clone(&sources[track.file_index]),
                track_layout.source_bytes.start,
                track_layout.source_bytes.len(),
            )
            .ok_or(PceCdLoadError::TrackOutsideBin(track.number))?;
            let built = if track_layout.virtual_pregap {
                CdTrack::from_index1_unverified_source(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    source,
                )
            } else {
                CdTrack::from_stored_unverified_source(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    source,
                )
            }
            .map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
            tracks.push(built);
        }
    }
    CdDisc::new(tracks).map_err(|error| PceCdLoadError::Disc(error.to_string()))
}

struct ByteSource {
    bytes: Arc<[u8]>,
    sha256: [u8; 32],
}

impl ByteSource {
    fn new(bytes: Arc<[u8]>) -> Self {
        let sha256 = Sha256::digest(&bytes).into();
        Self { bytes, sha256 }
    }
}

impl CdTrackSource for ByteSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn payload_hash(&self) -> [u8; 32] {
        self.sha256
    }

    fn read_exact_at(
        &self,
        offset: usize,
        output: &mut [u8],
    ) -> Result<(), zeff_pce_core::hardware::CdSourceError> {
        let end = offset
            .checked_add(output.len())
            .filter(|&end| end <= self.bytes.len())
            .ok_or(zeff_pce_core::hardware::CdSourceError::OutOfRange {
                offset,
                bytes: output.len(),
                source_len: self.bytes.len(),
            })?;
        output.copy_from_slice(&self.bytes[offset..end]);
        Ok(())
    }
}

fn patch_slot(relative: &str) -> Option<usize> {
    if relative.len() != 8 || &relative[4..] != ".ppf" {
        return None;
    }
    let digits = relative.as_bytes().get(..4)?;
    if digits[..3] != *b"000" || !(b'1'..=b'8').contains(&digits[3]) {
        return None;
    }
    Some((digits[3] - b'0') as usize)
}

fn update_identity(sha: &mut Sha256, crc: &mut Crc32, bytes: &[u8]) {
    let len = (bytes.len() as u64).to_le_bytes();
    sha.update(len);
    sha.update(bytes);
    crc.update(&len);
    crc.update(bytes);
}

fn sector_bytes(mode: CdTrackMode) -> usize {
    match mode {
        CdTrackMode::Mode1_2048 => 2_048,
        CdTrackMode::Mode1_2352 | CdTrackMode::Audio => 2_352,
    }
}

fn ppf_error(message: &str) -> PceCdLoadError {
    PceCdLoadError::Disc(format!("PC Engine CD archive PPF: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, size: u64) -> ArchivePpfMember {
        ArchivePpfMember {
            name: name.to_owned(),
            size,
            is_regular: true,
        }
    }

    #[test]
    fn discovery_is_ordered_contiguous_and_scoped_to_selected_cue() {
        let members = [
            member("elsewhere.ppf", 1),
            member("dir/other.ppf/0001.ppf", 1),
            member("dir/disc.ppf/0002.ppf", 2),
            member("dir/disc.ppf/0001.ppf", 1),
        ];
        assert_eq!(
            discover_archive_ppf_members("dir/disc.cue", &members).unwrap(),
            Some(vec![
                "dir/disc.ppf/0001.ppf".to_owned(),
                "dir/disc.ppf/0002.ppf".to_owned()
            ])
        );
        assert_eq!(
            discover_archive_ppf_members("dir/other.cue", &members).unwrap(),
            Some(vec!["dir/other.ppf/0001.ppf".to_owned()])
        );
    }

    #[test]
    fn discovery_rejects_gaps_malformed_and_non_regular_members() {
        assert!(
            discover_archive_ppf_members("disc.cue", &[member("disc.ppf/0002.ppf", 1)]).is_err()
        );
        assert!(
            discover_archive_ppf_members("disc.cue", &[member("disc.ppf/readme.txt", 1)]).is_err()
        );
        let mut directory = member("disc.ppf/0001.ppf", 0);
        directory.is_regular = false;
        assert!(discover_archive_ppf_members("disc.cue", &[directory]).is_err());
    }

    #[test]
    fn discovery_distinguishes_an_absent_container_from_an_empty_one() {
        assert_eq!(
            discover_archive_ppf_members("disc.cue", &[member("other.ppf", 1)]).unwrap(),
            None
        );
        let container = ArchivePpfMember {
            name: "disc.ppf".to_owned(),
            size: 0,
            is_regular: false,
        };
        assert!(discover_archive_ppf_members("disc.cue", &[container]).is_err());
    }

    #[test]
    fn discovery_rejects_oversized_and_noncanonical_patch_members() {
        assert!(
            discover_archive_ppf_members(
                "disc.cue",
                &[member(
                    "disc.ppf/0001.ppf",
                    ARCHIVE_PPF_PATCH_BYTES_LIMIT + 1,
                )],
            )
            .is_err()
        );
        assert!(
            discover_archive_ppf_members("disc.cue", &[member("disc.ppf/0001.PPF", 1)],).is_err()
        );
    }
}
