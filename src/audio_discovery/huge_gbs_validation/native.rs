use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;
use zeff_audio_discovery::huge::catalog::HugeSong;
use zeff_emu_common::debug::BusAccessEvent;
use zeff_gb_core::emulator::Emulator;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Call {
    pub cycles: u64,
    pub writes: Vec<(u64, u16, u8)>,
    pub ram: Vec<u8>,
}

pub fn collect(bytes: &[u8], song: &HugeSong, cancel: &AtomicBool) -> Result<Vec<Call>> {
    let mut emulator = Emulator::new(bytes, 48_000)?;
    let mut result = Vec::new();
    let mut active = None;
    let mut writes = Vec::new();
    while result.len() <= song.validation_frames as usize {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "native comparison cancelled"
        );
        ensure!(
            emulator.cpu_cycles() < (u64::from(song.validation_frames) + 1) * 70_224,
            "native calls exceeded their budget"
        );
        let pc = emulator.cpu_pc();
        let target = if result.is_empty() {
            song.bound.evidence.init_address
        } else {
            song.bound.evidence.update_address
        };
        if pc == target {
            ensure!(active.is_none(), "recursive driver entry");
            let sp = emulator.cpu_sp();
            let ret = u16::from_le_bytes([emulator.cpu_peek8(sp), emulator.cpu_peek8(sp + 1)]);
            active = Some((emulator.cpu_cycles(), sp + 2, ret));
            writes.clear();
        }
        let (_, opcode, _, _) = emulator.step_instruction_with_accesses(|event| {
            if let Some((base, _, _)) = active
                && let BusAccessEvent::Write {
                    at: Some(at),
                    addr,
                    written_value,
                    ..
                } = event
                && sound_address(addr as u16)
            {
                writes.push((at.get() - base, addr as u16, written_value as u8));
            }
        });
        if let Some((base, sp, ret)) = active
            && emulator.cpu_sp() == sp
            && emulator.cpu_pc() == ret
        {
            ensure!(
                matches!(opcode, 0xc0 | 0xc8 | 0xd0 | 0xd8 | 0xc9),
                "driver did not return with RET"
            );
            let ram = song.bound.evidence.ram_address;
            result.push(Call {
                cycles: emulator.cpu_cycles() - base,
                writes: std::mem::take(&mut writes),
                ram: (ram..ram + 100).map(|a| emulator.cpu_peek8(a)).collect(),
            });
            active = None;
        }
    }
    Ok(result)
}

pub fn sound_address(address: u16) -> bool {
    (0xff10..=0xff26).contains(&address) || (0xff30..=0xff3f).contains(&address)
}

pub fn normalized_ram(mut bytes: Vec<u8>, delta: u16) -> Result<Vec<u8>> {
    for offset in (1..=25).step_by(2) {
        let pointer = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        if pointer != 0 {
            let original = pointer
                .checked_sub(delta)
                .ok_or_else(|| anyhow::anyhow!("relocated RAM pointer underflows"))?;
            bytes[offset..offset + 2].copy_from_slice(&original.to_le_bytes());
        }
    }
    Ok(bytes)
}
