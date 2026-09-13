use anyhow::{Result, ensure};
use zeff_emu_common::system::System;

use super::{SegaPsgRegion, SegaPsgTiming, profiles::Profile};

const READY: u16 = 0xc000;
const ACK: u16 = 0xc001;
const BOOTSTRAP: usize = 0x100;

pub struct PreparedSegaPsg {
    pub bytes: Vec<u8>,
    pub system: System,
    pub region: SegaPsgRegion,
    pub timing: SegaPsgTiming,
    pub ready_address: u16,
    pub ack_address: u16,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub(super) fn prepare(bytes: &[u8], profile: &Profile, raw_index: u8) -> Result<PreparedSegaPsg> {
    ensure!(
        profile.audio_offset.is_multiple_of(0x4000)
            && matches!(profile.driver_address, 0x4000 | 0x8000)
            && profile.audio_offset >= 0x4000
            && matches!(profile.frame_divider, 1 | 2)
            && matches!(profile.audio_len(), 0x4000 | 0x8000)
            && usize::from(profile.driver_address) + profile.audio_len() <= 0xc000
            && bytes.len() == profile.rom_len
            && bytes.len() >= profile.audio_offset + profile.audio_len(),
        "unsupported Sega PSG bank mapping"
    );
    let bank = u8::try_from(profile.audio_offset / 0x4000)?;
    let (bank1, bank2) = if profile.driver_address == 0x4000 && profile.audio_len() == 0x4000 {
        (bank, 0)
    } else if profile.driver_address == 0x4000 {
        (
            bank,
            bank.checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Sega PSG bank overflow"))?,
        )
    } else {
        (1, bank)
    };
    // Stack and handshake storage stay below the driver's DE00..DF8F state.
    let mut code = vec![
        0xf3, 0x31, 0xf0, 0xdd, 0xaf, 0x32, 0xfc, 0xff, 0x32, 0xfd, 0xff,
    ];
    code.extend([0x3e, bank1, 0x32, 0xfe, 0xff, 0x3e, bank2, 0x32, 0xff, 0xff]);
    code.extend([
        0x21, 0, 0xc0, 0x11, 1, 0xc0, 1, 0xff, 0x1f, 0x36, 0, 0xed, 0xb0,
    ]);
    call(&mut code, profile.init_address);
    if profile.system == System::Gg {
        code.extend([0x3e, 0xff, 0xd3, 0x06]);
    }
    code.extend([0x3e, 1, 0x32, READY as u8, (READY >> 8) as u8]);
    let wait_start = u16::try_from(BOOTSTRAP + code.len())?;
    code.extend([0x3a, ACK as u8, (ACK >> 8) as u8, 0xfe, 1, 0xc2]);
    code.extend(wait_start.to_le_bytes());
    let wait_end = u16::try_from(BOOTSTRAP + code.len())?;
    code.extend([0x3e, raw_index, 0x32, 0x04, 0xde]);
    code.extend([0x3e, 4, 0xd3, 0xbf, 0x3e, 0x80, 0xd3, 0xbf]);
    code.extend([0x3e, 0x20, 0xd3, 0xbf, 0x3e, 0x81, 0xd3, 0xbf]);
    code.extend([0xdb, 0xbf, 0xed, 0x56, 0xfb]);
    let halt = u16::try_from(BOOTSTRAP + code.len())?;
    code.extend([0x76, 0xc3]);
    code.extend(halt.to_le_bytes());
    ensure!(
        BOOTSTRAP + code.len() <= 0x300,
        "Sega PSG bootstrap is too large"
    );
    let mut output = bytes.to_vec();
    output[..3].copy_from_slice(&[0xc3, 0, 1]);
    output[BOOTSTRAP..BOOTSTRAP + code.len()].copy_from_slice(&code);
    // The interrupted program is only the idle HALT loop.
    let mut irq = vec![0xdb, 0xbf];
    if profile.frame_divider == 2 {
        irq.extend([0x3a, 2, 0xc0, 0xee, 1, 0x32, 2, 0xc0, 0x20, 3]);
    }
    call(&mut irq, profile.driver_address);
    irq.extend([0xfb, 0xed, 0x4d]);
    output[0x38..0x38 + irq.len()].copy_from_slice(&irq);
    output[0x66..0x68].copy_from_slice(&[0xed, 0x45]);
    Ok(PreparedSegaPsg {
        bytes: output,
        system: profile.system,
        region: profile.region,
        timing: SegaPsgTiming::Ntsc,
        ready_address: READY,
        ack_address: ACK,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    code.push(0xcd);
    code.extend(address.to_le_bytes());
}
