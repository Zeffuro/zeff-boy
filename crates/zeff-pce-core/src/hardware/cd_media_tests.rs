use super::{
    CD_RAW_SECTOR_BYTES, CD_USER_SECTOR_BYTES, CdDisc, CdDiscError, CdReadError, CdSourceError,
    CdTrack, CdTrackMode, CdTrackSource,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct SyntheticSource {
    data: Box<[u8]>,
    payload_hash: [u8; 32],
    reads: Mutex<Vec<(usize, usize)>>,
    fail: AtomicBool,
}

impl SyntheticSource {
    fn new(data: Vec<u8>) -> Self {
        let payload_hash = Sha256::digest(&data).into();
        Self {
            data: data.into_boxed_slice(),
            payload_hash,
            reads: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }
}

impl CdTrackSource for SyntheticSource {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }

    fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(CdSourceError::ReadFailed);
        }
        let end = offset
            .checked_add(buffer.len())
            .filter(|&end| end <= self.data.len())
            .ok_or(CdSourceError::OutOfRange {
                offset,
                bytes: buffer.len(),
                source_len: self.data.len(),
            })?;
        buffer.copy_from_slice(&self.data[offset..end]);
        self.reads.lock().unwrap().push((offset, buffer.len()));
        Ok(())
    }
}

#[test]
fn mode1_2048_and_2352_return_exact_user_payloads() {
    let direct = vec![0x31; CD_USER_SECTOR_BYTES];
    let mut raw = vec![0xE0; CD_RAW_SECTOR_BYTES];
    raw[16..16 + CD_USER_SECTOR_BYTES].fill(0x62);
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, direct).unwrap(),
        CdTrack::from_index1_data(2, 4, Some(1), 1, CdTrackMode::Mode1_2352, raw).unwrap(),
    ])
    .unwrap();

    assert_eq!(
        disc.read_user_sector(0).unwrap(),
        [0x31; CD_USER_SECTOR_BYTES]
    );
    assert_eq!(
        disc.read_user_sector(1).unwrap(),
        [0x62; CD_USER_SECTOR_BYTES]
    );
    assert_eq!(disc.first_track(), 1);
    assert_eq!(disc.last_track(), 2);
    assert_eq!(disc.leadout_lba(), 2);
    assert_eq!(disc.track(2).unwrap().index0_lba(), Some(1));
}

#[test]
fn audio_and_out_of_range_reads_are_typed() {
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(
            1,
            0,
            None,
            10,
            CdTrackMode::Audio,
            vec![0; CD_RAW_SECTOR_BYTES],
        )
        .unwrap(),
    ])
    .unwrap();

    assert_eq!(
        disc.read_user_sector(10),
        Err(CdReadError::AudioTrack { lba: 10, track: 1 })
    );
    assert_eq!(disc.read_user_sector(9), Err(CdReadError::LbaOutOfRange(9)));
}

#[test]
fn retained_index_zero_payloads_are_addressable_with_exact_offsets() {
    let first = CdTrack::from_index1_data(
        1,
        4,
        None,
        0,
        CdTrackMode::Mode1_2048,
        vec![0x11; CD_USER_SECTOR_BYTES],
    )
    .unwrap();
    let mut data = vec![0; 2 * CD_RAW_SECTOR_BYTES];
    data[16..16 + CD_USER_SECTOR_BYTES].fill(0x20);
    data[CD_RAW_SECTOR_BYTES + 16..CD_RAW_SECTOR_BYTES + 16 + CD_USER_SECTOR_BYTES].fill(0x21);
    let data = CdTrack::from_stored_data(2, 4, Some(1), 2, CdTrackMode::Mode1_2352, data).unwrap();
    let mut audio = vec![0; 2 * CD_RAW_SECTOR_BYTES];
    audio[..2].copy_from_slice(&0x3030_i16.to_le_bytes());
    audio[2..4].copy_from_slice(&(-0x3030_i16).to_le_bytes());
    audio[CD_RAW_SECTOR_BYTES..CD_RAW_SECTOR_BYTES + 2].copy_from_slice(&0x4040_i16.to_le_bytes());
    audio[CD_RAW_SECTOR_BYTES + 2..CD_RAW_SECTOR_BYTES + 4]
        .copy_from_slice(&(-0x4040_i16).to_le_bytes());
    let audio = CdTrack::from_stored_data(3, 0, Some(3), 4, CdTrackMode::Audio, audio).unwrap();
    let disc = CdDisc::new(vec![first, data, audio]).unwrap();

    assert_eq!(disc.timeline_track_at_lba(1).unwrap().number(), 2);
    assert_eq!(disc.timeline_track_at_lba(3).unwrap().number(), 3);
    assert_eq!(disc.read_user_sector(1).unwrap()[0], 0x20);
    assert_eq!(disc.read_user_sector(2).unwrap()[0], 0x21);
    assert_eq!(disc.read_audio_sample(3, 0).unwrap(), (0x3030, -0x3030));
    assert_eq!(disc.read_audio_sample(4, 0).unwrap(), (0x4040, -0x4040));
    assert_eq!(disc.leadout_lba(), 5);
}

#[test]
fn track_adjacency_uses_the_next_retained_index_zero_boundary() {
    let first = CdTrack::from_index1_data(
        1,
        4,
        None,
        0,
        CdTrackMode::Mode1_2048,
        vec![0; 2 * CD_USER_SECTOR_BYTES],
    )
    .unwrap();
    let second = CdTrack::from_stored_data(
        2,
        0,
        Some(1),
        2,
        CdTrackMode::Audio,
        vec![0; 2 * CD_RAW_SECTOR_BYTES],
    )
    .unwrap();
    assert_eq!(
        CdDisc::new(vec![first, second]),
        Err(super::CdDiscError::OverlappingTracks {
            first: 1,
            second: 2
        })
    );

    let omitted = CdTrack::from_index1_data(
        1,
        0,
        Some(8),
        10,
        CdTrackMode::Audio,
        vec![0; CD_RAW_SECTOR_BYTES],
    )
    .unwrap();
    let disc = CdDisc::new(vec![omitted]).unwrap();
    assert_eq!(disc.track(1).unwrap().stored_start_lba(), 10);
    assert_eq!(disc.timeline_track_at_lba(9).unwrap().number(), 1);
    assert_eq!(
        disc.read_audio_sample(9, 0),
        Err(CdReadError::LbaOutOfRange(9))
    );
}

#[test]
fn source_tracks_preserve_exact_v1_identity() {
    let data = (0..CD_USER_SECTOR_BYTES)
        .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
        .collect::<Vec<_>>();
    let source = Arc::new(SyntheticSource::new(data.clone()));
    let source_disc = CdDisc::new(vec![
        CdTrack::from_index1_source(1, 4, None, 0, CdTrackMode::Mode1_2048, source.clone())
            .unwrap(),
    ])
    .unwrap();
    let memory_disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, data).unwrap(),
    ])
    .unwrap();

    assert_eq!(source_disc, memory_disc);
    assert_eq!(
        source_disc.read_user_sector(0),
        memory_disc.read_user_sector(0)
    );
    assert_eq!(
        source_disc.track(1).unwrap().payload_hash(),
        [
            0xEB, 0xDF, 0x6E, 0x59, 0x99, 0xBE, 0x27, 0x2C, 0x66, 0x88, 0x1A, 0xDF, 0x12, 0x35,
            0x8E, 0x34, 0x13, 0x85, 0xEC, 0x8F, 0x29, 0x1C, 0xEC, 0xAF, 0x29, 0x3A, 0xF9, 0xFB,
            0x8B, 0x16, 0x6C, 0x54,
        ]
    );
    assert_eq!(
        source_disc.content_hash(),
        [
            0xEB, 0x98, 0x9F, 0xFF, 0xE3, 0x9F, 0x70, 0xB7, 0x6D, 0x90, 0x15, 0x26, 0xF4, 0xEF,
            0x4E, 0x8E, 0x95, 0x93, 0xC9, 0x12, 0xC7, 0xF0, 0x5E, 0xBF, 0x08, 0x3C, 0xAC, 0x92,
            0xE3, 0x8F, 0xDC, 0x7B,
        ]
    );
    assert_eq!(
        *source.reads.lock().unwrap(),
        [(0, CD_USER_SECTOR_BYTES), (0, CD_USER_SECTOR_BYTES)]
    );
}

#[test]
fn unverified_source_hash_is_derived_during_the_canonical_scan() {
    let data = (0..2 * CD_USER_SECTOR_BYTES)
        .map(|index| (index as u8).wrapping_mul(19).wrapping_add(7))
        .collect::<Vec<_>>();
    let mut unverified = SyntheticSource::new(data.clone());
    unverified.payload_hash = [0; 32];
    let source = Arc::new(unverified);
    let source_disc = CdDisc::new(vec![
        CdTrack::from_stored_unverified_source(
            1,
            4,
            None,
            0,
            CdTrackMode::Mode1_2048,
            source.clone(),
        )
        .unwrap(),
    ])
    .unwrap();
    let memory_disc = CdDisc::new(vec![
        CdTrack::from_stored_data(1, 4, None, 0, CdTrackMode::Mode1_2048, data).unwrap(),
    ])
    .unwrap();

    assert_eq!(source_disc, memory_disc);
    assert_ne!(source_disc.track(1).unwrap().payload_hash(), [0; 32]);
    assert_eq!(
        *source.reads.lock().unwrap(),
        [
            (0, CD_USER_SECTOR_BYTES),
            (CD_USER_SECTOR_BYTES, CD_USER_SECTOR_BYTES)
        ]
    );
}

#[test]
fn unverified_index1_source_hash_is_derived_during_the_canonical_scan() {
    let data = vec![0x5a; CD_USER_SECTOR_BYTES];
    let mut unverified = SyntheticSource::new(data.clone());
    unverified.payload_hash = [0; 32];
    let source = Arc::new(unverified);
    let source_disc = CdDisc::new(vec![
        CdTrack::from_index1_unverified_source(
            1,
            4,
            None,
            0,
            CdTrackMode::Mode1_2048,
            source.clone(),
        )
        .unwrap(),
    ])
    .unwrap();
    let memory_disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, data).unwrap(),
    ])
    .unwrap();

    assert_eq!(source_disc, memory_disc);
    assert_eq!(*source.reads.lock().unwrap(), [(0, CD_USER_SECTOR_BYTES)]);
}

#[test]
fn source_reads_are_bounded_and_clones_share_the_source() {
    let mut data = vec![0; 2 * CD_RAW_SECTOR_BYTES];
    let offset = CD_RAW_SECTOR_BYTES + 587 * 4;
    data[offset..offset + 2].copy_from_slice(&0x1234_i16.to_le_bytes());
    data[offset + 2..offset + 4].copy_from_slice(&(-0x2345_i16).to_le_bytes());
    let source = Arc::new(SyntheticSource::new(data.clone()));
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_source(1, 0, None, 0, CdTrackMode::Audio, source.clone()).unwrap(),
    ])
    .unwrap();
    let memory_disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, data).unwrap(),
    ])
    .unwrap();

    assert_eq!(
        *source.reads.lock().unwrap(),
        [
            (0, CD_RAW_SECTOR_BYTES),
            (CD_RAW_SECTOR_BYTES, CD_RAW_SECTOR_BYTES)
        ]
    );
    source.reads.lock().unwrap().clear();
    assert_eq!(
        disc.read_audio_sample(1, 587),
        memory_disc.read_audio_sample(1, 587)
    );
    assert_eq!(disc.read_audio_sample(1, 587), Ok((0x1234, -0x2345)));
    assert_eq!(*source.reads.lock().unwrap(), [(offset, 4), (offset, 4)]);

    let cloned = disc.clone();
    source.fail.store(true, Ordering::Relaxed);
    assert_eq!(
        cloned.read_audio_sample(1, 587),
        Err(CdReadError::Source {
            lba: 1,
            track: 1,
            source: CdSourceError::ReadFailed,
        })
    );
}

#[test]
fn source_failures_and_hash_mismatches_are_typed() {
    let source = Arc::new(SyntheticSource::new(vec![0x55; CD_USER_SECTOR_BYTES]));
    let track = CdTrack::from_index1_source(1, 4, None, 0, CdTrackMode::Mode1_2048, source.clone())
        .unwrap();
    source.fail.store(true, Ordering::Relaxed);
    assert_eq!(
        CdDisc::new(vec![track]),
        Err(CdDiscError::Source {
            track: 1,
            source: CdSourceError::ReadFailed,
        })
    );

    let mut mismatched = SyntheticSource::new(vec![0xAA; CD_USER_SECTOR_BYTES]);
    mismatched.payload_hash = [0; 32];
    let track =
        CdTrack::from_index1_source(2, 4, None, 0, CdTrackMode::Mode1_2048, Arc::new(mismatched))
            .unwrap();
    assert_eq!(
        CdDisc::new(vec![track]),
        Err(CdDiscError::PayloadHashMismatch(2))
    );

    let mut buffer = [0xCC; 8];
    assert_eq!(
        source.read_exact_at(0, &mut buffer),
        Err(CdSourceError::ReadFailed)
    );
    assert_eq!(buffer, [0xCC; 8]);
}
