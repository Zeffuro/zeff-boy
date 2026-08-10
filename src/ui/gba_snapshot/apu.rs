pub(super) fn gba_apu_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
) -> crate::debug::ApuDebugInfo {
    let apu = emu.apu_debug_snapshot();
    let soundcnt_h = read_io16(emu, 0x0400_0082);
    let soundcnt_x = read_io16(emu, 0x0400_0084);
    let master_enabled = soundcnt_x & 0x0080 != 0;
    let mut channels = Vec::with_capacity(6);
    for index in 0..4 {
        let detail_line = match index {
            0 => format!("freq={} volume={}", apu.psg_frequency[0], apu.psg_volume[0]),
            1 => format!("freq={} volume={}", apu.psg_frequency[1], apu.psg_volume[1]),
            2 => format!("freq={} level={}", apu.psg_frequency[2], apu.psg_volume[2]),
            _ => format!("volume={}", apu.psg_volume[3]),
        };
        channels.push(crate::debug::ApuChannelDebug {
            name: match index {
                0 => "PSG 1",
                1 => "PSG 2",
                2 => "PSG 3",
                _ => "PSG 4",
            },
            enabled: apu.psg_enabled[index],
            muted: apu.channel_mutes[index],
            register_lines: vec!["GB/CGB-compatible PSG backend".into()],
            detail_line,
            waveform: Vec::new(),
        });
    }
    channels.push(gba_direct_sound_channel(
        "FIFO A",
        0,
        4,
        apu,
        soundcnt_h,
        master_enabled,
    ));
    channels.push(gba_direct_sound_channel(
        "FIFO B",
        1,
        5,
        apu,
        soundcnt_h,
        master_enabled,
    ));

    crate::debug::ApuDebugInfo {
        master_lines: vec![
            format!(
                "SOUNDCNT_H={soundcnt_h:04X} SOUNDCNT_X={soundcnt_x:04X} master={}",
                if master_enabled { "on" } else { "off" }
            ),
            format!(
                "sample_rate={} pending_samples={} generation={}",
                apu.sample_rate,
                apu.sample_buffer_len,
                if apu.sample_generation_enabled {
                    "on"
                } else {
                    "off"
                }
            ),
        ],
        master_waveform: Vec::new(),
        channels,
        extra_sections: vec![crate::debug::DebugSection {
            heading: "GBA APU",
            lines: vec![
                "PSG uses the existing GB/CGB APU backend; Direct Sound FIFO A/B is implemented in the GBA wrapper.".into(),
            ],
        }],
    }
}

fn gba_direct_sound_channel(
    name: &'static str,
    fifo: usize,
    mute_index: usize,
    apu: zeff_gba_core::hardware::apu::ApuDebugSnapshot,
    soundcnt_h: u16,
    master_enabled: bool,
) -> crate::debug::ApuChannelDebug {
    let (right_bit, left_bit, timer_bit, volume_bit) = if fifo == 0 {
        (8, 9, 10, 2)
    } else {
        (12, 13, 14, 3)
    };
    let right = soundcnt_h & (1 << right_bit) != 0;
    let left = soundcnt_h & (1 << left_bit) != 0;
    let timer = if soundcnt_h & (1 << timer_bit) != 0 {
        1
    } else {
        0
    };
    let volume = if soundcnt_h & (1 << volume_bit) != 0 {
        "100%"
    } else {
        "50%"
    };
    crate::debug::ApuChannelDebug {
        name,
        enabled: master_enabled && (left || right),
        muted: apu.channel_mutes[mute_index],
        register_lines: vec![format!(
            "fifo={} current={} timer={} volume={} route={}",
            apu.fifo_len[fifo],
            apu.current_sample[fifo],
            timer,
            volume,
            route_label(left, right)
        )],
        detail_line: String::new(),
        waveform: Vec::new(),
    }
}

fn read_io16(emu: &zeff_gba_core::emulator::Emulator, addr: u32) -> u16 {
    u16::from_le_bytes([emu.cpu_peek8(addr), emu.cpu_peek8(addr + 1)])
}

fn route_label(left: bool, right: bool) -> &'static str {
    match (left, right) {
        (true, true) => "L+R",
        (true, false) => "L",
        (false, true) => "R",
        (false, false) => "-",
    }
}
