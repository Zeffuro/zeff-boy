use std::{io::Write, sync::atomic::AtomicBool};

use flate2::{Compression, write::GzEncoder};

use super::*;
use crate::{
    ScanLimits,
    catalog::SongRef,
    formats::{AudioFormat, SongFormat},
    test_support::vgm::put32,
};

fn playable(commands: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 0x80];
    bytes[..4].copy_from_slice(b"Vgm ");
    put32(&mut bytes, 8, 0x171);
    put32(&mut bytes, 0x34, 0x80 - 0x34);
    put32(&mut bytes, 0x0c, 3_579_545);
    bytes[0x28..0x2a].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x2a] = 16;
    bytes[0x2b] = 0;
    bytes.extend_from_slice(commands);
    let samples = commands.iter().fold(0u64, |samples, &opcode| match opcode {
        0x62 => samples + 735,
        0x63 => samples + 882,
        0x70..=0x7f => samples + u64::from((opcode & 15) + 1),
        _ => samples,
    });
    put32(&mut bytes, 0x18, samples as u32);
    let eof = bytes.len() as u32 - 4;
    put32(&mut bytes, 4, eof);
    bytes
}

fn log(bytes: &[u8]) -> VgmLog {
    super::super::inspect(
        bytes,
        ScanLimits {
            max_work: 20_000,
            max_candidates: 1,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn prepares_ordered_writes_and_exact_duration() {
    let mut bytes = playable(&[
        0x50, 0x91, 0x4f, 0xff, 0x70, 0x50, 0x92, 0x61, 2, 0, 0x62, 0x63, 0x66,
    ]);
    put32(&mut bytes, 0x18, 1 + 2 + 735 + 882);
    let inventory = log(&bytes);
    assert_eq!(
        inventory.sn_playback,
        Some(SnPlayback {
            clock_hz: 3_579_545,
            model: SnPsgModel::Sega,
            stereo: true
        })
    );
    let prepared = prepare(&bytes, &inventory, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.duration_ticks, 1620);
    assert_eq!(
        prepared.writes,
        [
            TimedSnWrite {
                tick: 0,
                write: SnWrite::Psg(0x91)
            },
            TimedSnWrite {
                tick: 0,
                write: SnWrite::Stereo(0xff)
            },
            TimedSnWrite {
                tick: 1,
                write: SnWrite::Psg(0x92)
            },
        ]
    );
}

#[test]
fn accepts_gzip_ti_and_capture_sega_clocks() {
    for clock in [3_579_545, 3_546_893, 3_584_160, 3_568_200] {
        let mut bytes = playable(&[0x50, 0x90, 0x62, 0x66]);
        put32(&mut bytes, 0x0c, clock);
        assert_eq!(log(&bytes).sn_playback.unwrap().model, SnPsgModel::Sega);
    }
    let mut ti = playable(&[0x50, 0x90, 0x62, 0x66]);
    ti[0x28..0x2a].copy_from_slice(&3u16.to_le_bytes());
    ti[0x2a] = 15;
    ti[0x2b] = 5;
    assert_eq!(log(&ti).sn_playback.unwrap().model, SnPsgModel::TiSn76489);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&ti).unwrap();
    let gzip = encoder.finish().unwrap();
    let inventory = log(&gzip);
    assert_eq!(
        prepare(&gzip, &inventory, &AtomicBool::new(false))
            .unwrap()
            .duration_ticks,
        735
    );
}

#[test]
fn short_headers_do_not_treat_command_bytes_as_header_fields() {
    let mut bytes = vec![0; 0x40];
    bytes[..4].copy_from_slice(b"Vgm ");
    put32(&mut bytes, 8, 0x171);
    put32(&mut bytes, 0x0c, 3_579_545);
    bytes[0x28..0x2a].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x2a] = 16;
    bytes.extend(std::iter::repeat_n(0x70, 59));
    bytes.extend_from_slice(&[0x50, 0x90, 0x66]);
    put32(&mut bytes, 0x18, 59);
    let eof = bytes.len() as u32 - 4;
    put32(&mut bytes, 4, eof);
    assert_eq!(bytes[0x7c], 0x90);
    assert!(log(&bytes).sn_playback.is_some());
}

#[test]
fn rejects_unqualified_headers_commands_and_duration() {
    let valid = playable(&[0x50, 0x90, 0x62, 0x66]);
    for mutate in [
        Box::new(|bytes: &mut Vec<u8>| put32(bytes, 8, 0x160)) as Box<dyn Fn(&mut Vec<u8>)>,
        Box::new(|bytes: &mut Vec<u8>| put32(bytes, 0x0c, 3_000_000)),
        Box::new(|bytes: &mut Vec<u8>| put32(bytes, 0x0c, 0x4036_9e99)),
        Box::new(|bytes: &mut Vec<u8>| bytes[0x2b] = 1),
        Box::new(|bytes: &mut Vec<u8>| bytes[0x7c] = 1),
        Box::new(|bytes: &mut Vec<u8>| put32(bytes, 0x10, 3_579_545)),
        Box::new(|bytes: &mut Vec<u8>| bytes[0x80] = 0x51),
        Box::new(|bytes: &mut Vec<u8>| put32(bytes, 0x18, 1)),
    ] {
        let mut bytes = valid.clone();
        mutate(&mut bytes);
        assert!(log(&bytes).sn_playback.is_none());
    }
    let no_writes = playable(&[0x62, 0x66]);
    assert!(log(&no_writes).sn_playback.is_none());
    let mut mono_stereo = playable(&[0x4f, 0xff, 0x50, 0x90, 0x62, 0x66]);
    mono_stereo[0x2b] = 4;
    assert!(log(&mono_stereo).sn_playback.is_none());
}

#[test]
fn preparation_rechecks_source_and_cancellation() {
    let bytes = playable(&[0x50, 0x90, 0x62, 0x66]);
    let inventory = log(&bytes);
    let mut changed = bytes.clone();
    changed[0x81] ^= 1;
    assert!(prepare(&changed, &inventory, &AtomicBool::new(false)).is_err());
    assert!(prepare(&bytes, &inventory, &AtomicBool::new(true)).is_err());
}

#[test]
fn catalog_audio_support_follows_the_playback_capability() {
    let qualified = log(&playable(&[0x50, 0x90, 0x62, 0x66]));
    let unqualified = log(&playable(&[0x62, 0x66]));
    for format in [
        SongFormat::Audio(AudioFormat::Wav),
        SongFormat::Audio(AudioFormat::Flac),
        SongFormat::Audio(AudioFormat::Ogg),
    ] {
        assert!(SongRef::Vgm(&qualified).supports(format));
        assert!(!SongRef::Vgm(&unqualified).supports(format));
    }
    for log in [&qualified, &unqualified] {
        assert!(SongRef::Vgm(log).supports(SongFormat::Vgm));
        assert!(SongRef::Vgm(log).supports(SongFormat::MappedAssets));
    }
}
