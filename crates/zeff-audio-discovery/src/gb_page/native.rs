use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbPageSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbPageSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let p = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Page-state native profile is not recognized"))?;
    let mut output = bytes.to_vec();
    let mut code = vec![
        0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07,
    ];
    if p.double {
        code.extend([0x3e, 1, 0xe0, 0x4d, 0x10, 0]);
    }
    code.extend([0xaf, 0xea, 0, 0x30, 0x3e, 1, 0xea, 0, 0x20, 0xe0, 0x70]);
    clear(&mut code, p.page, 17);
    if p.wrapper != 0 {
        clear(&mut code, p.wrapper, 6);
        code.extend([0x3e, 1, 0xe0, 0x8d]);
    }
    code.extend([0xaf, 0xe0, 0xfb, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfb, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    call(&mut code, p.setup);
    call(&mut code, p.stop);
    if p.wrapper != 0 {
        let entry = song.table_entry.canonical_cpu_address as u16;
        code.extend([0x21, entry as u8, (entry >> 8) as u8]);
    } else {
        code.extend([
            0x06,
            song.bank as u8,
            0x11,
            song.module_address as u8,
            (song.module_address >> 8) as u8,
        ]);
    }
    call(&mut code, p.start);
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "Page-state bootstrap exceeds reserved space"
    );
    for &(a, b) in p.protected {
        for (start, end) in [
            (0x100, 0x103),
            (0x40, 0x43),
            (0x150, 0x150 + code.len()),
            (0x200, 0x20c),
        ] {
            anyhow::ensure!(
                end <= usize::from(a) || start >= usize::from(b),
                "Page-state bootstrap overlaps qualified native code or data"
            );
        }
    }
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    let tick = p.tick.to_le_bytes();
    output[0x200..0x20c].copy_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, tick[0], tick[1], 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: if p.double {
            GbBankedTiming::CgbDouble
        } else {
            GbBankedTiming::Cgb
        },
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffb,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    code.extend([0xcd, address as u8, (address >> 8) as u8]);
}

fn clear(code: &mut Vec<u8>, start: u16, len: u16) {
    code.extend([
        0x21,
        start as u8,
        (start >> 8) as u8,
        0x01,
        len as u8,
        (len >> 8) as u8,
        0xaf,
        0x22,
        0x0b,
        0x78,
        0xb1,
        0x20,
        0xf9,
    ]);
}
