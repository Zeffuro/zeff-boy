use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::apu::{PSG_CHANNEL_COUNT, PSG_TONE_CHANNEL_COUNT};

pub(super) fn sega8_apu_snapshot(emu: &Emulator) -> ApuDebugInfo {
    let apu = emu.bus().apu().debug_snapshot();
    let channels = (0..PSG_CHANNEL_COUNT)
        .map(|index| ApuChannelDebug {
            name: channel_name(index),
            enabled: apu.volume[index] < 15,
            muted: apu.channel_mutes[index],
            register_lines: vec![channel_register_line(index, &apu)],
            detail_line: channel_detail_line(index, &apu, emu.video_standard().clock_hz_approx()),
            waveform: Vec::new(),
        })
        .collect();

    ApuDebugInfo {
        master_lines: vec![
            format!(
                "stereo={:02X} latched={} last_write={} writes={}",
                apu.stereo_control,
                apu.latched_register,
                apu.last_write
                    .map(|value| format!("{value:02X}"))
                    .unwrap_or_else(|| "--".into()),
                apu.write_count
            ),
            format!(
                "sample_rate={} buffered_samples={} generation={}",
                apu.sample_rate,
                apu.buffered_samples,
                super::on_off(apu.sample_generation_enabled)
            ),
        ],
        master_waveform: Vec::new(),
        channels,
        extra_sections: vec![DebugSection {
            heading: "Stereo Routing",
            lines: (0..PSG_CHANNEL_COUNT)
                .map(|index| {
                    format!(
                        "{}: {}",
                        channel_name(index),
                        stereo_route(index, apu.stereo_control)
                    )
                })
                .collect(),
        }],
    }
}

fn channel_register_line(
    index: usize,
    apu: &zeff_sega8_core::hardware::apu::ApuDebugSnapshot,
) -> String {
    if index < PSG_TONE_CHANNEL_COUNT {
        format!(
            "tone_period={:03X} volume={:X} route={}",
            apu.tone_period[index],
            apu.volume[index],
            stereo_route(index, apu.stereo_control)
        )
    } else {
        format!(
            "noise_control={:02X} volume={:X} route={}",
            apu.noise_control,
            apu.volume[index],
            stereo_route(index, apu.stereo_control)
        )
    }
}

fn channel_detail_line(
    index: usize,
    apu: &zeff_sega8_core::hardware::apu::ApuDebugSnapshot,
    clock_hz: u32,
) -> String {
    if index < PSG_TONE_CHANNEL_COUNT {
        format!(
            "freq={}Hz",
            tone_frequency_label(apu.tone_period[index], clock_hz)
        )
    } else {
        format!(
            "mode={} shift_rate={}",
            if apu.noise_control & 0x04 != 0 {
                "white"
            } else {
                "periodic"
            },
            apu.noise_control & 0x03
        )
    }
}

fn tone_frequency_label(period: u16, clock_hz: u32) -> String {
    if period == 0 {
        "-".into()
    } else {
        format!("{:.1}", f64::from(clock_hz) / 16.0 / f64::from(period))
    }
}

fn channel_name(index: usize) -> &'static str {
    match index {
        0 => "Tone 0",
        1 => "Tone 1",
        2 => "Tone 2",
        _ => "Noise",
    }
}

fn stereo_route(index: usize, stereo_control: u8) -> &'static str {
    let right = stereo_control & (1 << index) != 0;
    let left = stereo_control & (1 << (index + 4)) != 0;
    match (left, right) {
        (true, true) => "L+R",
        (true, false) => "L",
        (false, true) => "R",
        (false, false) => "-",
    }
}
