use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use zeff_gb_core::save_state::SaveState;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Control {
    pc: u16,
    sp: u16,
    registers: [u8; 8],
    ram: Vec<u8>,
    ie: u8,
    interrupt_flags: u8,
    nr51: u8,
    timer_control: u8,
    cycle_phase: u64,
}

impl Control {
    pub fn capture(state: &SaveState, ram: u16) -> Self {
        let r = &state.cpu.regs;
        Self {
            pc: state.cpu.pc,
            sp: state.cpu.sp,
            registers: [r.a, r.f, r.b, r.c, r.d, r.e, r.h, r.l],
            ram: (ram..ram + 100).map(|a| state.bus.read_byte(a)).collect(),
            ie: state.bus.ie,
            interrupt_flags: state.bus.if_reg,
            nr51: state.bus.read_byte(0xff25),
            timer_control: state.bus.read_byte(0xff07),
            cycle_phase: state.cpu.cycles % 70_224,
        }
    }
}

pub fn validate(
    states: &[(usize, Control)],
    accesses: &[String],
    loop_ticks: u32,
) -> Result<Value> {
    let period = zeff_audio_discovery::huge::catalog::control_period_frames(loop_ticks)
        .ok_or_else(|| anyhow::anyhow!("invalid GBS recurrence period"))? as usize;
    let start = loop_ticks as usize;
    ensure!(
        states.len() == 3
            && states[0].0 == start
            && states[1].0 == start + period
            && states[2].0 == start + period * 2,
        "missing GBS recurrence boundaries"
    );
    ensure!(
        states[0].1 == states[1].1 && states[1].1 == states[2].1,
        "GBS caller/driver control state does not recur"
    );
    let first = accesses
        .get(start + 1..start + period + 1)
        .ok_or_else(|| anyhow::anyhow!("missing first GBS period"))?;
    let second = accesses
        .get(start + period + 1..start + 2 * period + 1)
        .ok_or_else(|| anyhow::anyhow!("missing second GBS period"))?;
    ensure!(first == second, "GBS consumed accesses do not repeat");
    Ok(json!({"passed":true,"period_frames":period,
        "boundaries":states.iter().map(|(at, state)| json!({"completed_updates":at,
            "state_sha256":zeff_firmware::sha256_hex(&serde_json::to_vec(state).expect("serializable state"))})).collect::<Vec<_>>(),
        "period_accesses_sha256":zeff_firmware::sha256_hex(&serde_json::to_vec(first)?),
        "excluded":"APU oscillator/envelope/filter phase; no seamless PCM loop claim"}))
}
