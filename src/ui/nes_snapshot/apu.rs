use crate::debug::{ApuChannelDebug, ApuDebugInfo};

pub(super) fn nes_apu_snapshot(
    emu: &zeff_nes_core::emulator::Emulator,
    show: bool,
) -> Option<ApuDebugInfo> {
    if !show {
        return None;
    }

    let apu = &emu.bus().apu;
    let muted = apu.channel_mutes();
    let master_lines = vec![
        format!(
            "Frame mode:{}  IRQ inhibit:{}  Frame IRQ:{}",
            if apu.five_step_mode {
                "5-step"
            } else {
                "4-step"
            },
            apu.irq_inhibit,
            apu.frame_irq
        ),
        format!(
            "Sample rate:{:.1} Hz  Buffered samples:{}",
            apu.output_sample_rate,
            apu.sample_buffer.len()
        ),
    ];

    let channels = vec![
        ApuChannelDebug {
            name: "Pulse 1".into(),
            enabled: apu.pulse1.midi_active(),
            muted: muted[0],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X} Vol:{}",
                apu.pulse1.length_counter,
                apu.pulse1.timer_period(),
                apu.pulse1.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(0),
        },
        ApuChannelDebug {
            name: "Pulse 2".into(),
            enabled: apu.pulse2.midi_active(),
            muted: muted[1],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X} Vol:{}",
                apu.pulse2.length_counter,
                apu.pulse2.timer_period(),
                apu.pulse2.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(1),
        },
        ApuChannelDebug {
            name: "Triangle".into(),
            enabled: apu.triangle.midi_active(),
            muted: muted[2],
            register_lines: vec![format!(
                "Len:{} Timer:{:03X}",
                apu.triangle.length_counter,
                apu.triangle.timer_period()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(2),
        },
        ApuChannelDebug {
            name: "Noise".into(),
            enabled: apu.noise.midi_active(),
            muted: muted[3],
            register_lines: vec![format!(
                "Len:{} Vol:{}",
                apu.noise.length_counter,
                apu.noise.midi_volume()
            )],
            detail_line: String::new(),
            waveform: apu.channel_debug_samples_ordered(3),
        },
    ];

    Some(ApuDebugInfo {
        master_lines,
        master_waveform: apu.master_debug_samples_ordered(),
        channels,
        extra_sections: Vec::new(),
    })
}
