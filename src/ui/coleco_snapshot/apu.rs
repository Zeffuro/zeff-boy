use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_coleco_core::Emulator;
use zeff_coleco_core::psg::{COLECO_PSG_INPUT_CLOCK_HZ, PSG_TONE_CHANNEL_COUNT, PsgDebugSnapshot};

pub(super) fn coleco_apu_snapshot(emu: &Emulator) -> ApuDebugInfo {
    let snapshot = emu.bus().psg().debug_snapshot();
    ApuDebugInfo {
        master_lines: vec![
            format!(
                "latched={} last_write={} writes={} ready={} hold={}",
                snapshot.latched_register,
                snapshot
                    .last_write
                    .map(|value| format!("{value:02X}"))
                    .unwrap_or_else(|| "--".into()),
                snapshot.write_count,
                on_off(snapshot.ready),
                snapshot.ready_clocks_remaining
            ),
            format!(
                "sample_rate={} buffered_samples={} generation={} muted={}",
                snapshot.sample_rate,
                snapshot.buffered_sample_count,
                on_off(snapshot.sample_generation_enabled),
                on_off(snapshot.muted)
            ),
        ],
        master_waveform: Vec::new(),
        channels: (0..4)
            .map(|index| channel_snapshot(index, snapshot))
            .collect(),
        extra_sections: vec![DebugSection {
            heading: "Noise",
            lines: vec![format!(
                "control={:02X} counter={:03X} lfsr={:04X}",
                snapshot.noise_control, snapshot.noise_counter, snapshot.noise_lfsr
            )],
        }],
    }
}

fn channel_snapshot(index: usize, snapshot: PsgDebugSnapshot) -> ApuChannelDebug {
    let tone = index < PSG_TONE_CHANNEL_COUNT;
    let volume = snapshot.volumes[index];
    ApuChannelDebug {
        name: channel_name(index),
        enabled: volume < 15,
        muted: snapshot.channel_mutes[index],
        register_lines: vec![if tone {
            format!(
                "tone_period={:03X} counter={:03X} volume={:X}",
                snapshot.tone_periods[index], snapshot.tone_counters[index], volume
            )
        } else {
            format!(
                "noise_control={:02X} volume={:X}",
                snapshot.noise_control, volume
            )
        }],
        detail_line: if tone {
            format!(
                "freq={:.1}Hz output={}",
                tone_frequency(snapshot.effective_tone_periods[index]),
                on_off(snapshot.tone_output_high[index])
            )
        } else {
            format!(
                "mode={} shift_rate={}",
                if snapshot.noise_control & 0x04 != 0 {
                    "white"
                } else {
                    "periodic"
                },
                snapshot.noise_control & 0x03
            )
        },
        waveform: Vec::new(),
    }
}

fn tone_frequency(period: u16) -> f64 {
    f64::from(COLECO_PSG_INPUT_CLOCK_HZ) / (32.0 * f64::from(period.max(1)))
}

fn channel_name(index: usize) -> &'static str {
    match index {
        0 => "Tone 0",
        1 => "Tone 1",
        2 => "Tone 2",
        _ => "Noise",
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
