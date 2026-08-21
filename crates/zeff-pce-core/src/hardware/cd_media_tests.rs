use super::{CD_RAW_SECTOR_BYTES, CD_USER_SECTOR_BYTES, CdDisc, CdReadError, CdTrack, CdTrackMode};

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
