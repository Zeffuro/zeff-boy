use std::path::PathBuf;

use zeff_gb_core::hardware::apu::ApuChannelSnapshot as GbApuChannelSnapshot;
use zeff_nes_core::hardware::apu::ApuChannelSnapshot as NesApuChannelSnapshot;

use super::MidiApuSnapshot;

pub(super) fn gb_square_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    131072.0 / denom as f64
}

pub(super) fn gb_wave_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    65536.0 / denom as f64
}

pub(super) fn hz_to_midi_note(hz: f64) -> u8 {
    if hz <= 0.0 {
        return 0;
    }
    let note = 69.0 + 12.0 * (hz / 440.0).log2();
    (note.round() as i32).clamp(0, 127) as u8
}

pub(super) fn volume_to_velocity(vol: u8) -> u8 {
    ((vol as u16 * 127 + 7) / 15).min(127) as u8
}

pub(super) fn wave_level_to_velocity(level: u8) -> u8 {
    match level {
        0 => 0,
        1 => 127,
        2 => 80,
        3 => 48,
        _ => 0,
    }
}

pub(super) fn finish_midi(
    path: PathBuf,
    snapshots: &[MidiApuSnapshot],
) -> std::io::Result<PathBuf> {
    if snapshots.is_empty() {
        std::fs::write(&path, [])?;
        return Ok(path);
    }

    const TICKS_PER_FRAME: u16 = 1;
    const GB_TEMPO_US_PER_BEAT: u32 = 16742;
    const NES_TEMPO_US_PER_BEAT: u32 = 16639;

    let is_nes = matches!(snapshots[0], MidiApuSnapshot::Nes(_));
    let track_data: Vec<Vec<u8>>;
    let tempo_us: u32;

    if is_nes {
        let nes_snapshots: Vec<NesApuChannelSnapshot> = snapshots
            .iter()
            .filter_map(|s| match s {
                MidiApuSnapshot::Nes(snap) => Some(*snap),
                MidiApuSnapshot::Gb(_) => None,
            })
            .collect();
        tempo_us = NES_TEMPO_US_PER_BEAT;
        track_data = (0..4)
            .map(|ch| build_midi_track_nes(&nes_snapshots, ch))
            .collect();
    } else {
        let gb_snapshots: Vec<GbApuChannelSnapshot> = snapshots
            .iter()
            .filter_map(|s| match s {
                MidiApuSnapshot::Gb(snap) => Some(*snap),
                MidiApuSnapshot::Nes(_) => None,
            })
            .collect();
        tempo_us = GB_TEMPO_US_PER_BEAT;
        track_data = (0..4)
            .map(|ch| build_midi_track_gb(&gb_snapshots, ch))
            .collect();
    }

    let mut smf = Vec::with_capacity(snapshots.len() * 16);

    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes());
    smf.extend_from_slice(&1u16.to_be_bytes());
    smf.extend_from_slice(&5u16.to_be_bytes());
    smf.extend_from_slice(&TICKS_PER_FRAME.to_be_bytes());

    let tempo_track = build_tempo_track(tempo_us);
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(tempo_track.len() as u32).to_be_bytes());
    smf.extend_from_slice(&tempo_track);

    for track in &track_data {
        smf.extend_from_slice(b"MTrk");
        smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
        smf.extend_from_slice(track);
    }

    std::fs::write(&path, &smf)?;
    Ok(path)
}

fn build_tempo_track(tempo_us: u32) -> Vec<u8> {
    let mut data = Vec::new();

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x51);
    data.push(0x03);
    data.push((tempo_us >> 16) as u8);
    data.push((tempo_us >> 8) as u8);
    data.push(tempo_us as u8);

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

fn midi_program_for_channel(channel: usize) -> Option<u8> {
    match channel {
        0 | 1 => Some(80),
        2 => Some(81),
        3 => None,
        _ => None,
    }
}

fn drum_note_for_noise(volume: u8) -> u8 {
    match volume {
        0..=4 => 36,
        5..=9 => 38,
        _ => 42,
    }
}

pub(super) fn nes_pulse_freq_to_hz(timer_period: u16) -> f64 {
    zeff_nes_core::hardware::constants::APU_CPU_CLOCK_NTSC / (16.0 * (timer_period as f64 + 1.0))
}

fn nes_triangle_freq_to_hz(timer_period: u16) -> f64 {
    zeff_nes_core::hardware::constants::APU_CPU_CLOCK_NTSC / (32.0 * (timer_period as f64 + 1.0))
}

struct FrameChannelState {
    note: u8,
    velocity: u8,
    enabled: bool,
}

fn build_midi_track(
    name: &str,
    channel: usize,
    frame_count: usize,
    mut extract: impl FnMut(usize) -> FrameChannelState,
) -> Vec<u8> {
    let mut data = Vec::new();

    let midi_ch: u8 = if channel == 3 { 9 } else { channel as u8 };

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x03);
    write_vlq(&mut data, name.len() as u32);
    data.extend_from_slice(name.as_bytes());

    if let Some(program) = midi_program_for_channel(channel) {
        write_vlq(&mut data, 0);
        data.push(0xC0 | midi_ch);
        data.push(program);
    }

    let mut current_note: Option<u8> = None;
    let mut current_velocity: u8 = 0;
    let mut pending_delta: u32 = 0;

    for i in 0..frame_count {
        let state = extract(i);
        let should_sound = state.enabled && state.velocity > 0;

        if should_sound {
            if let Some(prev_note) = current_note {
                if prev_note != state.note {
                    // Note changed:note-off then note-on
                    write_vlq(&mut data, pending_delta);
                    data.push(0x80 | midi_ch);
                    data.push(prev_note);
                    data.push(0);
                    pending_delta = 0;

                    write_vlq(&mut data, 0);
                    data.push(0x90 | midi_ch);
                    data.push(state.note);
                    data.push(state.velocity);

                    current_note = Some(state.note);
                    current_velocity = state.velocity;
                } else if state.velocity != current_velocity && channel != 3 {
                    // Same note, velocity changed:aftertouch
                    write_vlq(&mut data, pending_delta);
                    data.push(0xA0 | midi_ch);
                    data.push(state.note);
                    data.push(state.velocity);
                    pending_delta = 0;
                    current_velocity = state.velocity;
                }
            } else {
                // New note on
                write_vlq(&mut data, pending_delta);
                data.push(0x90 | midi_ch);
                data.push(state.note);
                data.push(state.velocity);
                pending_delta = 0;
                current_note = Some(state.note);
                current_velocity = state.velocity;
            }
        } else if let Some(prev_note) = current_note.take() {
            // Sound stopped:note off
            write_vlq(&mut data, pending_delta);
            data.push(0x80 | midi_ch);
            data.push(prev_note);
            data.push(0);
            pending_delta = 0;
            current_velocity = 0;
        }

        pending_delta = pending_delta.saturating_add(1);
    }

    // Final note-off if still sounding
    if let Some(prev_note) = current_note {
        write_vlq(&mut data, pending_delta);
        data.push(0x80 | midi_ch);
        data.push(prev_note);
        data.push(0);
    }

    // End of track
    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

pub(super) fn build_midi_track_gb(snapshots: &[GbApuChannelSnapshot], channel: usize) -> Vec<u8> {
    let name = match channel {
        0 => "GB CH1 (Square 1)",
        1 => "GB CH2 (Square 2)",
        2 => "GB CH3 (Wave)",
        3 => "GB CH4 (Noise)",
        _ => "Unknown",
    };

    build_midi_track(name, channel, snapshots.len(), |i| {
        let snap = &snapshots[i];
        let (enabled, freq_reg, vol_raw) = match channel {
            0 => (snap.ch1_enabled, snap.ch1_frequency, snap.ch1_volume),
            1 => (snap.ch2_enabled, snap.ch2_frequency, snap.ch2_volume),
            2 => (snap.ch3_enabled, snap.ch3_frequency, 0),
            3 => (snap.ch4_enabled, 0, snap.ch4_volume),
            _ => (false, 0, 0),
        };

        let velocity = if channel == 2 {
            wave_level_to_velocity(snap.ch3_output_level)
        } else {
            volume_to_velocity(vol_raw)
        };

        let note = if channel == 3 {
            drum_note_for_noise(vol_raw)
        } else {
            let hz = if channel == 2 {
                gb_wave_freq_to_hz(freq_reg)
            } else {
                gb_square_freq_to_hz(freq_reg)
            };
            hz_to_midi_note(hz)
        };

        FrameChannelState {
            note,
            velocity,
            enabled,
        }
    })
}

pub(super) fn build_midi_track_nes(snapshots: &[NesApuChannelSnapshot], channel: usize) -> Vec<u8> {
    let name = match channel {
        0 => "NES Pulse 1",
        1 => "NES Pulse 2",
        2 => "NES Triangle",
        3 => "NES Noise",
        _ => "Unknown",
    };

    build_midi_track(name, channel, snapshots.len(), |i| {
        let snap = &snapshots[i];
        let (enabled, timer_period, vol_raw) = match channel {
            0 => (
                snap.pulse1_enabled,
                snap.pulse1_timer_period,
                snap.pulse1_volume,
            ),
            1 => (
                snap.pulse2_enabled,
                snap.pulse2_timer_period,
                snap.pulse2_volume,
            ),
            2 => (
                snap.triangle_enabled,
                snap.triangle_timer_period,
                snap.triangle_volume,
            ),
            3 => (snap.noise_enabled, 0, snap.noise_volume),
            _ => (false, 0, 0),
        };

        let velocity = volume_to_velocity(vol_raw);
        let note = if channel == 3 {
            drum_note_for_noise(vol_raw)
        } else {
            let hz = if channel == 2 {
                nes_triangle_freq_to_hz(timer_period)
            } else {
                nes_pulse_freq_to_hz(timer_period)
            };
            hz_to_midi_note(hz)
        };

        FrameChannelState {
            note,
            velocity,
            enabled,
        }
    })
}

pub(super) fn write_vlq(buf: &mut Vec<u8>, mut value: u32) {
    if value == 0 {
        buf.push(0);
        return;
    }

    let mut bytes = [0u8; 4];
    let mut count = 0;

    while value > 0 {
        bytes[count] = (value & 0x7F) as u8;
        value >>= 7;
        count += 1;
    }

    for i in (1..count).rev() {
        buf.push(bytes[i] | 0x80);
    }
    buf.push(bytes[0]);
}
