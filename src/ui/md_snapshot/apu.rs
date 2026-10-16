use crate::debug::{ApuDebugInfo, DebugSection};
use zeff_md_core::Emulator;

pub(super) fn snapshot(emu: &Emulator) -> ApuDebugInfo {
    let psg = emu.psg_snapshot();
    let fm = emu.opn2_snapshot();
    ApuDebugInfo {
        master_lines: vec![
            format!("sample_rate={}", emu.sample_rate()),
            "Read-only current state; sample history and channel controls unavailable".into(),
        ],
        master_waveform: Vec::new(),
        channels: Vec::new(),
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
