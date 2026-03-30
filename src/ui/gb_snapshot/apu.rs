use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_gb_core::emulator::Emulator;

pub(super) fn gb_apu_snapshot(emu: &Emulator, show: bool) -> Option<ApuDebugInfo> {
    if !show {
        return None;
    }
    use zeff_gb_core::hardware::types::constants::*;

    let regs = emu.apu_regs_snapshot();
    let wave_ram = emu.apu_wave_ram_snapshot();
    let nr52 = emu.apu_nr52_raw();
    let channel_samples = [
        emu.apu_channel_debug_samples_ordered(0),
        emu.apu_channel_debug_samples_ordered(1),
        emu.apu_channel_debug_samples_ordered(2),
        emu.apu_channel_debug_samples_ordered(3),
    ];
    let master_samples = emu.apu_master_debug_samples_ordered();
    let muted = emu.apu_channel_mutes();

    let ri = |addr: u16| (addr - NR10) as usize;
    let duty = |val: u8| match (val >> 6) & 0x03 {
        0 => "12.5%",
        1 => "25%",
        2 => "50%",
        3 => "75%",
        _ => "?",
    };

    let master_lines = vec![
        format!(
            "NR50:{:02X}  NR51:{:02X}  NR52:{:02X}",
            regs[ri(NR50)],
            regs[ri(NR51)],
            nr52
        ),
        format!(
            "Power:{}  CH1:{} CH2:{} CH3:{} CH4:{}",
            if nr52 & 0x80 != 0 { "ON" } else { "OFF" },
            if nr52 & 0x01 != 0 { "1" } else { "-" },
            if nr52 & 0x02 != 0 { "1" } else { "-" },
            if nr52 & 0x04 != 0 { "1" } else { "-" },
            if nr52 & 0x08 != 0 { "1" } else { "-" },
        ),
    ];

    let channels = vec![
        ApuChannelDebug {
            name: "CH1 (Square + Sweep)".into(),
            enabled: nr52 & 0x01 != 0,
            muted: muted[0],
            register_lines: vec![format!(
                "NR10:{:02X} NR11:{:02X} NR12:{:02X} NR13:{:02X} NR14:{:02X}",
                regs[ri(NR10)],
                regs[ri(NR11)],
                regs[ri(NR12)],
                regs[ri(NR13)],
                regs[ri(NR14)]
            )],
            detail_line: format!(
                "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
                duty(regs[ri(NR11)]),
                regs[ri(NR11)] & 0x3F,
                regs[ri(NR12)] >> 4,
                if regs[ri(NR12)] & 0x08 != 0 { "+" } else { "-" },
                regs[ri(NR12)] & 0x07,
                (u16::from(regs[ri(NR14)] & 0x07) << 8) | u16::from(regs[ri(NR13)])
            ),
            waveform: channel_samples[0].to_vec(),
        },
        ApuChannelDebug {
            name: "CH2 (Square)".into(),
            enabled: nr52 & 0x02 != 0,
            muted: muted[1],
            register_lines: vec![format!(
                "NR21:{:02X} NR22:{:02X} NR23:{:02X} NR24:{:02X}",
                regs[ri(NR21)],
                regs[ri(NR22)],
                regs[ri(NR23)],
                regs[ri(NR24)]
            )],
            detail_line: format!(
                "Duty:{} Len:{} Vol:{} Env:{} P:{} Freq:{:03X}",
                duty(regs[ri(NR21)]),
                regs[ri(NR21)] & 0x3F,
                regs[ri(NR22)] >> 4,
                if regs[ri(NR22)] & 0x08 != 0 { "+" } else { "-" },
                regs[ri(NR22)] & 0x07,
                (u16::from(regs[ri(NR24)] & 0x07) << 8) | u16::from(regs[ri(NR23)])
            ),
            waveform: channel_samples[1].to_vec(),
        },
        ApuChannelDebug {
            name: "CH3 (Wave)".into(),
            enabled: nr52 & 0x04 != 0,
            muted: muted[2],
            register_lines: vec![format!(
                "NR30:{:02X} NR31:{:02X} NR32:{:02X} NR33:{:02X} NR34:{:02X}",
                regs[ri(NR30)],
                regs[ri(NR31)],
                regs[ri(NR32)],
                regs[ri(NR33)],
                regs[ri(NR34)]
            )],
            detail_line: format!(
                "DAC:{} Len:{} Level:{} Freq:{:03X}",
                if regs[ri(NR30)] & 0x80 != 0 {
                    "ON"
                } else {
                    "OFF"
                },
                regs[ri(NR31)],
                (regs[ri(NR32)] >> 5) & 0x03,
                (u16::from(regs[ri(NR34)] & 0x07) << 8) | u16::from(regs[ri(NR33)])
            ),
            waveform: channel_samples[2].to_vec(),
        },
        ApuChannelDebug {
            name: "CH4 (Noise)".into(),
            enabled: nr52 & 0x08 != 0,
            muted: muted[3],
            register_lines: vec![format!(
                "NR41:{:02X} NR42:{:02X} NR43:{:02X} NR44:{:02X}",
                regs[ri(NR41)],
                regs[ri(NR42)],
                regs[ri(NR43)],
                regs[ri(NR44)]
            )],
            detail_line: format!(
                "Len:{} Vol:{} Env:{} P:{} Poly(s={},w={},r={})",
                regs[ri(NR41)] & 0x3F,
                regs[ri(NR42)] >> 4,
                if regs[ri(NR42)] & 0x08 != 0 { "+" } else { "-" },
                regs[ri(NR42)] & 0x07,
                regs[ri(NR43)] >> 4,
                if regs[ri(NR43)] & 0x08 != 0 { "7" } else { "15" },
                regs[ri(NR43)] & 0x07
            ),
            waveform: channel_samples[3].to_vec(),
        },
    ];

    let wave_lines: Vec<String> = wave_ram
        .chunks(4)
        .map(|chunk| {
            chunk
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();

    Some(ApuDebugInfo {
        master_lines,
        master_waveform: master_samples.to_vec(),
        channels,
        extra_sections: vec![DebugSection {
            heading: "Wave RAM".into(),
            lines: wave_lines,
        }],
    })
}

