use crate::debug::{ApuChannelDebug, ApuDebugInfo, DebugSection};
use zeff_md_core::Emulator;

pub(super) fn snapshot(emu: &Emulator) -> ApuDebugInfo {
    let psg = emu.psg_snapshot();
    let fm = emu.opn2_snapshot();
    let mutes = emu.apu_channel_mutes();
    let channels = [
        "PSG Tone 1",
        "PSG Tone 2",
        "PSG Tone 3",
        "PSG Noise",
        "FM 1",
        "FM 2",
        "FM 3",
        "FM 4",
        "FM 5",
        "FM 6 / DAC",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, name)| {
        let (enabled, registers, detail) = if index < 3 {
            (
                psg.volumes[index] < 15,
                format!(
                    "period={:03X} attenuation={:X} counter={:03X} output={}",
                    psg.periods[index], psg.volumes[index], psg.counters[index], psg.outputs[index]
                ),
                "ON means attenuation is below maximum".to_owned(),
            )
        } else if index == 3 {
            (
                psg.volumes[index] < 15,
                format!(
                    "noise_control={:X} attenuation={:X} counter={:04X} clock={} lfsr={:04X}",
                    psg.noise_control,
                    psg.volumes[index],
                    psg.noise_counter,
                    psg.noise_clock,
                    psg.noise_lfsr
                ),
                "ON means attenuation is below maximum".to_owned(),
            )
        } else {
            let output = fm.output[index - 4];
            let dac = index == 9 && fm.dac_enabled;
            let registers = if dac {
                format!(
                    "source=DAC dac={:02X} bypassed_fm_output={output}",
                    fm.dac
                )
            } else {
                format!("instantaneous_output={output}")
            };
            let detail = if dac {
                "DAC mode selected (ON); FM 6 output bypassed; mute/solo shares both sources"
            } else if index == 9 {
                "Mute/solo shares FM 6 and DAC; ON means nonzero instantaneous FM output, not key-on"
            } else {
                "ON means nonzero instantaneous FM output, not key-on"
            };
            (dac || output != 0, registers, detail.to_owned())
        };
        ApuChannelDebug {
            name,
            enabled,
            muted: mutes[index],
            register_lines: vec![registers],
            detail_line: detail,
            waveform: Vec::new(),
        }
    })
    .collect();
    ApuDebugInfo {
        master_lines: vec![
            format!("sample_rate={}", emu.sample_rate()),
            "Current chip state; sample history unavailable. Mute/solo changes host output only."
                .into(),
        ],
        master_waveform: Vec::new(),
        channels,
        extra_sections: vec![
            DebugSection {
                heading: "PSG",
                lines: vec![
                    format!(
                        "periods={:03X?} attenuation={:X?}",
                        psg.periods, psg.volumes
                    ),
                    format!("counters={:03X?} outputs={:?}", psg.counters, psg.outputs),
                    format!(
                        "latched_register={:X} noise_control={:X}",
                        psg.latched_register, psg.noise_control
                    ),
                    format!(
                        "noise_counter={:04X} noise_clock={} noise_lfsr={:04X}",
                        psg.noise_counter, psg.noise_clock, psg.noise_lfsr
                    ),
                ],
            },
            DebugSection {
                heading: "OPN2",
                lines: vec![
                    format!(
                        "address={:03X} status={:02X} mode={:02X}",
                        fm.address, fm.status, fm.mode
                    ),
                    format!("timer_a={:03X} timer_b={:02X}", fm.timer_a, fm.timer_b),
                    format!("dac={:02X} dac_enabled={}", fm.dac, fm.dac_enabled),
                    format!("instantaneous_outputs={:?}", fm.output),
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests;
