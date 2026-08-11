use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_gba_core::emulator::Emulator;
use zeff_gba_core::hardware::apu::ApuDebugSnapshot;

const NR10: u16 = 0xFF10;
const NR11: u16 = 0xFF11;
const NR12: u16 = 0xFF12;
const NR13: u16 = 0xFF13;
const NR14: u16 = 0xFF14;
const NR21: u16 = 0xFF16;
const NR22: u16 = 0xFF17;
const NR23: u16 = 0xFF18;
const NR24: u16 = 0xFF19;
const NR30: u16 = 0xFF1A;
const NR31: u16 = 0xFF1B;
const NR32: u16 = 0xFF1C;
const NR33: u16 = 0xFF1D;
const NR34: u16 = 0xFF1E;
const NR41: u16 = 0xFF20;
const NR42: u16 = 0xFF21;
const NR43: u16 = 0xFF22;
const NR44: u16 = 0xFF23;
const NR50: u16 = 0xFF24;
const NR51: u16 = 0xFF25;

pub(super) fn gba_apu_snapshot(emu: &Emulator) -> ApuDebugInfo {
    let apu = emu.apu_debug_snapshot();
    let soundcnt_l = read_io16(emu, 0x0400_0080);
    let soundcnt_h = read_io16(emu, 0x0400_0082);
    let soundcnt_x = read_io16(emu, 0x0400_0084);
    let soundbias = read_io16(emu, 0x0400_0088);
    let master_enabled = soundcnt_x & 0x0080 != 0;
    let regs = emu.apu_psg_regs_snapshot();
    let wave_ram = emu.apu_psg_wave_ram_snapshot();
    let nr52 = emu.apu_psg_nr52_raw();

    let mut channels = Vec::with_capacity(6);
    for index in 0..4 {
        channels.push(gba_psg_channel(
            index,
            &apu,
            &regs,
            nr52,
            emu.apu_psg_channel_debug_samples_ordered(index).to_vec(),
        ));
    }
    channels.push(gba_direct_sound_channel(
        "FIFO A",
        0,
        &apu,
        soundcnt_h,
        soundcnt_x,
        emu.apu_direct_debug_samples_ordered(0).to_vec(),
    ));
    channels.push(gba_direct_sound_channel(
        "FIFO B",
        1,
        &apu,
        soundcnt_h,
        soundcnt_x,
        emu.apu_direct_debug_samples_ordered(1).to_vec(),
    ));

    ApuDebugInfo {
        master_lines: vec![
            format!(
                "SOUNDCNT_L={soundcnt_l:04X} SOUNDCNT_H={soundcnt_h:04X} SOUNDCNT_X={soundcnt_x:04X} SOUNDBIAS={soundbias:04X}"
            ),
            format!(
                "NR50:{:02X} NR51:{:02X} NR52:{nr52:02X} master={} psg_mix={}",
                regs[ri(NR50)],
                regs[ri(NR51)],
                on_off(master_enabled),
                gba_psg_mix_volume(soundcnt_h)
            ),
            format!(
                "sample_rate={} psg_rate={} pending_samples={} generation={} capture={}",
                apu.sample_rate,
                apu.psg_sample_rate,
                apu.sample_buffer_len,
                on_off(apu.sample_generation_enabled),
                on_off(apu.debug_capture_enabled)
            ),
            format!(
                "pairs output={} direct={} psg={}",
                apu.output_pairs_generated, apu.direct_pairs_generated, apu.psg_pairs_generated
            ),
        ],
        master_waveform: emu.apu_master_debug_samples_ordered().to_vec(),
        channels,
        extra_sections: vec![
            DebugSection {
                heading: "PSG Wave RAM",
                lines: wave_ram_lines(&wave_ram),
            },
            DebugSection {
                heading: "GBA APU Notes",
                lines: vec![
                    "PSG channels use the GB/CGB-compatible backend; FIFO A/B are GBA Direct Sound channels.".into(),
                    "Master waveform is the final filtered GBA output; PSG waveforms are captured before GBA Direct Sound mixing.".into(),
                    format!(
                        "PSG master captured samples={}",
                        emu.apu_psg_master_debug_samples_ordered().len()
                    ),
                ],
            },
        ],
    }
}

fn gba_psg_channel(
    index: usize,
    apu: &ApuDebugSnapshot,
    regs: &[u8; 0x17],
    nr52: u8,
    waveform: Vec<f32>,
) -> ApuChannelDebug {
    let register_lines = match index {
        0 => vec![format!(
            "NR10:{:02X} NR11:{:02X} NR12:{:02X} NR13:{:02X} NR14:{:02X}",
            regs[ri(NR10)],
            regs[ri(NR11)],
            regs[ri(NR12)],
            regs[ri(NR13)],
            regs[ri(NR14)]
        )],
        1 => vec![format!(
            "NR21:{:02X} NR22:{:02X} NR23:{:02X} NR24:{:02X}",
            regs[ri(NR21)],
            regs[ri(NR22)],
            regs[ri(NR23)],
            regs[ri(NR24)]
        )],
        2 => vec![format!(
            "NR30:{:02X} NR31:{:02X} NR32:{:02X} NR33:{:02X} NR34:{:02X}",
            regs[ri(NR30)],
            regs[ri(NR31)],
            regs[ri(NR32)],
            regs[ri(NR33)],
            regs[ri(NR34)]
        )],
        _ => vec![format!(
            "NR41:{:02X} NR42:{:02X} NR43:{:02X} NR44:{:02X}",
            regs[ri(NR41)],
            regs[ri(NR42)],
            regs[ri(NR43)],
            regs[ri(NR44)]
        )],
    };

    ApuChannelDebug {
        name: match index {
            0 => "PSG 1 (Square + Sweep)",
            1 => "PSG 2 (Square)",
            2 => "PSG 3 (Wave)",
            _ => "PSG 4 (Noise)",
        },
        enabled: apu.psg_enabled[index] || nr52 & (1 << index) != 0,
        muted: apu.channel_mutes[index],
        register_lines,
        detail_line: gba_psg_detail(index, regs),
        waveform,
    }
}

fn gba_psg_detail(index: usize, regs: &[u8; 0x17]) -> String {
    let nr51 = regs[ri(NR51)];
    match index {
        0 => format!(
            "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X} Route:{}",
            duty(regs[ri(NR11)]),
            regs[ri(NR11)] & 0x3F,
            regs[ri(NR12)] >> 4,
            envelope_dir(regs[ri(NR12)]),
            regs[ri(NR12)] & 0x07,
            frequency(regs[ri(NR13)], regs[ri(NR14)]),
            psg_route_label(nr51, index)
        ),
        1 => format!(
            "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X} Route:{}",
            duty(regs[ri(NR21)]),
            regs[ri(NR21)] & 0x3F,
            regs[ri(NR22)] >> 4,
            envelope_dir(regs[ri(NR22)]),
            regs[ri(NR22)] & 0x07,
            frequency(regs[ri(NR23)], regs[ri(NR24)]),
            psg_route_label(nr51, index)
        ),
        2 => format!(
            "DAC:{} Len:{} Level:{} Freq:{:03X} Route:{}",
            on_off(regs[ri(NR30)] & 0x80 != 0),
            regs[ri(NR31)],
            (regs[ri(NR32)] >> 5) & 0x03,
            frequency(regs[ri(NR33)], regs[ri(NR34)]),
            psg_route_label(nr51, index)
        ),
        _ => format!(
            "Len:{} Vol:{} Env:{} P:{} Poly(s={},w={},r={}) Route:{}",
            regs[ri(NR41)] & 0x3F,
            regs[ri(NR42)] >> 4,
            envelope_dir(regs[ri(NR42)]),
            regs[ri(NR42)] & 0x07,
            regs[ri(NR43)] >> 4,
            if regs[ri(NR43)] & 0x08 != 0 {
                "7"
            } else {
                "15"
            },
            regs[ri(NR43)] & 0x07,
            psg_route_label(nr51, index)
        ),
    }
}

fn gba_direct_sound_channel(
    name: &'static str,
    fifo: usize,
    apu: &ApuDebugSnapshot,
    soundcnt_h: u16,
    soundcnt_x: u16,
    waveform: Vec<f32>,
) -> ApuChannelDebug {
    let mute_index = 4 + fifo;
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
    ApuChannelDebug {
        name,
        enabled: soundcnt_x & 0x0080 != 0 && (left || right),
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
        waveform,
    }
}

fn read_io16(emu: &Emulator, addr: u32) -> u16 {
    u16::from_le_bytes([emu.cpu_peek8(addr), emu.cpu_peek8(addr + 1)])
}

fn wave_ram_lines(wave_ram: &[u8; 0x10]) -> Vec<String> {
    wave_ram
        .chunks(4)
        .map(|chunk| {
            chunk
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn ri(addr: u16) -> usize {
    (addr - NR10) as usize
}

fn duty(value: u8) -> &'static str {
    match (value >> 6) & 0x03 {
        0 => "12.5%",
        1 => "25%",
        2 => "50%",
        3 => "75%",
        _ => "?",
    }
}

fn envelope_dir(value: u8) -> &'static str {
    if value & 0x08 != 0 { "+" } else { "-" }
}

fn frequency(lo: u8, hi: u8) -> u16 {
    (u16::from(hi & 0x07) << 8) | u16::from(lo)
}

fn gba_psg_mix_volume(soundcnt_h: u16) -> &'static str {
    match soundcnt_h & 0x0003 {
        0 => "25%",
        1 => "50%",
        _ => "100%",
    }
}

fn psg_route_label(nr51: u8, index: usize) -> &'static str {
    let right = nr51 & (1 << index) != 0;
    let left = nr51 & (1 << (index + 4)) != 0;
    route_label(left, right)
}

fn route_label(left: bool, right: bool) -> &'static str {
    match (left, right) {
        (true, true) => "L+R",
        (true, false) => "L",
        (false, true) => "R",
        (false, false) => "-",
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
