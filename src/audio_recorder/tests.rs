use super::midi::*;
use super::*;

use zeff_gb_core::hardware::apu::ApuChannelSnapshot as GbApuChannelSnapshot;
use zeff_nes_core::hardware::apu::ApuChannelSnapshot as NesApuChannelSnapshot;

fn gb_snapshot(ch1_enabled: bool, ch1_frequency: u16, ch1_volume: u8) -> GbApuChannelSnapshot {
    GbApuChannelSnapshot {
        ch1_enabled,
        ch1_frequency,
        ch1_volume,
        ch2_enabled: false,
        ch2_frequency: 0,
        ch2_volume: 0,
        ch3_enabled: false,
        ch3_frequency: 0,
        ch3_output_level: 0,
        ch4_enabled: false,
        ch4_volume: 0,
    }
}

fn nes_snapshot(
    pulse1_enabled: bool,
    pulse1_timer_period: u16,
    pulse1_volume: u8,
) -> NesApuChannelSnapshot {
    NesApuChannelSnapshot {
        pulse1_enabled,
        pulse1_timer_period,
        pulse1_volume,
        pulse2_enabled: false,
        pulse2_timer_period: 0,
        pulse2_volume: 0,
        triangle_enabled: false,
        triangle_timer_period: 0,
        triangle_volume: 0,
        noise_enabled: false,
        noise_volume: 0,
    }
}

fn read_vlq(bytes: &[u8], i: &mut usize) -> u32 {
    let mut value = 0u32;
    loop {
        let b = bytes[*i];
        *i += 1;
        value = (value << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            break;
        }
    }
    value
}

fn ch0_note_events(track: &[u8]) -> Vec<(u32, u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < track.len() {
        let delta = read_vlq(track, &mut i);
        if i >= track.len() {
            break;
        }
        let status = track[i];
        i += 1;
        match status {
            0x90 | 0x80 => {
                if i + 1 >= track.len() {
                    break;
                }
                let note = track[i];
                let vel = track[i + 1];
                i += 2;
                out.push((delta, status, note));
                let _ = vel;
            }
            0xA0 => {
                i += 2;
            }
            0xC0 => {
                i += 1;
            }
            0xFF => {
                if i >= track.len() {
                    break;
                }
                let _meta = track[i];
                i += 1;
                let len = read_vlq(track, &mut i) as usize;
                i = i.saturating_add(len);
            }
            _ => break,
        }
    }
    out
}

#[test]
fn gb_square_freq_to_hz_middle_range() {
    let hz = super::midi::gb_square_freq_to_hz(1750);
    assert!((hz - 439.8).abs() < 1.0);
}

#[test]
fn gb_wave_freq_to_hz_middle_range() {
    let hz = super::midi::gb_wave_freq_to_hz(1750);
    assert!((hz - 219.9).abs() < 1.0);
}

#[test]
fn nes_pulse_freq_to_hz_middle_range() {
    let hz = super::midi::nes_pulse_freq_to_hz(253);
    assert!((hz - 440.0).abs() < 1.0);
}

#[test]
fn hz_to_midi_note_a4() {
    assert_eq!(hz_to_midi_note(440.0), 69);
}

#[test]
fn hz_to_midi_note_c4() {
    assert_eq!(hz_to_midi_note(261.63), 60);
}

#[test]
fn hz_to_midi_note_zero_returns_zero() {
    assert_eq!(hz_to_midi_note(0.0), 0);
}

#[test]
fn volume_to_velocity_full() {
    assert_eq!(volume_to_velocity(15), 127);
}

#[test]
fn volume_to_velocity_zero() {
    assert_eq!(volume_to_velocity(0), 0);
}

#[test]
fn wave_level_to_velocity_values() {
    assert_eq!(super::midi::wave_level_to_velocity(0), 0);
    assert_eq!(super::midi::wave_level_to_velocity(1), 127);
    assert_eq!(super::midi::wave_level_to_velocity(2), 80);
    assert_eq!(super::midi::wave_level_to_velocity(3), 48);
}

#[test]
fn write_vlq_zero() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0);
    assert_eq!(buf, vec![0]);
}

#[test]
fn write_vlq_small() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0x7F);
    assert_eq!(buf, vec![0x7F]);
}

#[test]
fn write_vlq_two_bytes() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0x80);
    assert_eq!(buf, vec![0x81, 0x00]);
}

#[test]
fn finish_midi_produces_valid_smf_header() {
    let snapshots = vec![
        MidiApuSnapshot::Gb(gb_snapshot(true, 1750, 15)),
        MidiApuSnapshot::Gb(gb_snapshot(true, 1800, 12)),
    ];

    let dir = std::env::temp_dir();
    let path = dir.join("test_midi_output.mid");
    let result = finish_midi(path.clone(), &snapshots);
    assert!(result.is_ok());

    let data = std::fs::read(&path).unwrap();
    assert!(data.len() > 14);
    assert_eq!(&data[0..4], b"MThd");
    assert_eq!(&data[14..18], b"MTrk");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn midi_note_change_on_adjacent_snapshots_advances_time() {
    let snapshots = vec![gb_snapshot(true, 1750, 15), gb_snapshot(true, 1800, 15)];

    let track = build_midi_track_gb(&snapshots, 0);
    let events = ch0_note_events(&track);
    assert!(events.len() >= 4);

    assert_eq!(events[0].0, 0);
    assert_eq!(events[0].1, 0x90);
    assert_eq!(events[1].0, 1);
    assert_eq!(events[1].1, 0x80);
    assert_eq!(events[2].0, 0);
    assert_eq!(events[2].1, 0x90);
}

#[test]
fn midi_note_off_on_adjacent_snapshot_advances_time() {
    let snapshots = vec![gb_snapshot(true, 1750, 15), gb_snapshot(false, 1750, 15)];

    let track = build_midi_track_gb(&snapshots, 0);
    let events = ch0_note_events(&track);
    assert!(events.len() >= 2);

    assert_eq!(events[0], (0, 0x90, events[0].2));
    assert_eq!(events[1], (1, 0x80, events[1].2));
}

#[test]
fn midi_new_note_after_one_silent_snapshot_uses_delta_one() {
    let snapshots = vec![gb_snapshot(false, 1750, 15), gb_snapshot(true, 1750, 15)];

    let track = build_midi_track_gb(&snapshots, 0);
    let events = ch0_note_events(&track);
    assert!(!events.is_empty());

    assert_eq!(events[0].0, 1);
    assert_eq!(events[0].1, 0x90);
}

#[test]
fn nes_midi_note_change_on_adjacent_snapshots_advances_time() {
    let snapshots = vec![nes_snapshot(true, 253, 15), nes_snapshot(true, 225, 15)];

    let track = build_midi_track_nes(&snapshots, 0);
    let events = ch0_note_events(&track);
    assert!(events.len() >= 4);

    assert_eq!(events[0].0, 0);
    assert_eq!(events[0].1, 0x90);
    assert_eq!(events[1].0, 1);
    assert_eq!(events[1].1, 0x80);
    assert_eq!(events[2].0, 0);
    assert_eq!(events[2].1, 0x90);
}
