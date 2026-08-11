use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_ws_core::emulator::Emulator;

pub(super) fn ws_apu_snapshot(emu: &Emulator) -> ApuDebugInfo {
    let apu = emu.apu_debug_snapshot();
    let channels = (0..4usize)
        .map(|index| {
            let volume = apu.volume[index];
            let left = volume >> 4;
            let right = volume & 0x0F;
            let enabled = apu.control & (1 << index) != 0;
            ApuChannelDebug {
                name: match index {
                    0 => "CH0 Wave",
                    1 => "CH1 Wave/Voice",
                    2 => "CH2 Wave/Sweep",
                    _ => "CH3 Wave/Noise",
                },
                enabled,
                muted: apu.channel_mutes[index],
                register_lines: vec![format!(
                    "period={:03X} volume={:02X} L={} R={} sample_pos={} mode={}",
                    apu.period[index],
                    volume,
                    left,
                    right,
                    apu.sample_pos[index],
                    ws_apu_channel_mode(index, apu.control, apu.noise_control)
                )],
                detail_line: format!(
                    "freq={}Hz {}",
                    ws_apu_frequency_label(apu.period[index]),
                    if enabled { "enabled" } else { "disabled" }
                ),
                waveform: ws_apu_channel_waveform(emu, apu.sample_ram_pos, index),
            }
        })
        .collect();

    ApuDebugInfo {
        master_lines: vec![
            format!(
                "CTRL={:02X} OUT={:02X} sample_base={:02X} buffered_samples={} generation={}",
                apu.control,
                apu.output_control,
                apu.sample_ram_pos,
                apu.buffered_samples,
                if apu.sample_generation_enabled {
                    "on"
                } else {
                    "off"
                }
            ),
            format!(
                "sample_rate={} noise={:02X} lfsr={:04X} sweep_value={:02X} sweep_step={:02X}",
                apu.sample_rate, apu.noise_control, apu.nreg, apu.sweep_value, apu.sweep_step
            ),
            format!(
                "voice_volume={:02X} hyper_sample={:02X} hyper_ctrl={:02X} hyper_ch={:02X} hyper_L={} hyper_R={} next={}",
                apu.voice_volume,
                apu.hyper_voice_sample,
                apu.hyper_voice_control,
                apu.hyper_voice_channel_control,
                apu.hyper_voice_left_output,
                apu.hyper_voice_right_output,
                if apu.hyper_voice_next_left {
                    "left"
                } else {
                    "right"
                }
            ),
        ],
        master_waveform: Vec::new(),
        channels,
        extra_sections: vec![DebugSection {
            heading: "Wave RAM",
            lines: ws_apu_wave_ram_lines(emu, apu.sample_ram_pos),
        }],
    }
}

fn ws_apu_channel_mode(channel: usize, control: u8, noise_control: u8) -> &'static str {
    match channel {
        1 if control & 0x20 != 0 => "direct voice",
        2 if control & 0x40 != 0 => "sweep",
        3 if control & 0x80 != 0 && noise_control & 0x10 != 0 => "noise",
        _ => "wave",
    }
}

fn ws_apu_frequency_label(period: u16) -> String {
    let clocks = 2048u16.saturating_sub(period & 0x07FF);
    if clocks <= 4 {
        "-".into()
    } else {
        format!("{:.1}", 3_072_000.0 / f64::from(clocks) / 32.0)
    }
}

fn ws_apu_channel_waveform(emu: &Emulator, sample_ram_pos: u8, channel: usize) -> Vec<f32> {
    (0..32usize)
        .map(|sample_pos| {
            let offset =
                (u32::from(sample_ram_pos) << 6) + (channel as u32) * 16 + (sample_pos as u32 / 2);
            let byte = emu.cpu_peek8(offset);
            let sample = if sample_pos & 1 == 0 {
                byte & 0x0F
            } else {
                byte >> 4
            };
            (f32::from(sample) - 7.5) / 7.5
        })
        .collect()
}

fn ws_apu_wave_ram_lines(emu: &Emulator, sample_ram_pos: u8) -> Vec<String> {
    (0..4u32)
        .map(|channel| {
            let base = (u32::from(sample_ram_pos) << 6) + channel * 16;
            let bytes = (0..16u32)
                .map(|offset| format!("{:02X}", emu.cpu_peek8(base + offset)))
                .collect::<Vec<_>>()
                .join(" ");
            format!("CH{channel}: {bytes}")
        })
        .collect()
}
