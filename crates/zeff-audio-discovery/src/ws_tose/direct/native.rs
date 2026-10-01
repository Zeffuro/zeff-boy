use super::*;

pub(in crate::ws_tose) fn prepare_rom(
    bytes: &[u8],
    song: &WsToseSong,
    cancel: &AtomicBool,
) -> Result<PreparedWsTose> {
    let profile = checked_profile(bytes, song, cancel)?;
    let driver = profile.driver;
    let (first, count) = selector(bytes, profile, song.index)
        .map_err(|_| anyhow::anyhow!("invalid WonderSwan direct selector"))?;
    let bootstrap = bytes.len() - 0x2000;
    ensure!(
        driver.segment == 0x5000
            && bytes.len() == 0x200000
            && u32::from(driver.slots) + 8 * 0x34 < 0x3d00
            && song.mapped_spans.iter().all(|span| {
                let start = span.effective_offset as usize;
                let end = start + span.byte_len as usize;
                end <= bootstrap || start >= bootstrap + 512
            }),
        "WonderSwan direct driver overlaps bootstrap storage"
    );
    let mut code = vec![
        0xfa, 0xfc, 0x33, 0xc0, 0x8e, 0xd0, 0xbc, 0xf0, 0x3d, 0x8e, 0xd8, 0x8e, 0xc0, 0x33, 0xff,
        0xb9, 0, 0x20, 0xf3, 0xab, 0xe6, 0xb2, 0xb0, 1, 0xe6, 0xc0,
    ];
    far_call(&mut code, driver.init);
    far_call(&mut code, driver.tick);
    far_call(&mut code, driver.status);
    far_call(&mut code, driver.init);
    code.extend([0xc6, 0x06, 0, 0x3e, 1]);
    let wait_start = 0xfe000 + code.len() as u32;
    code.extend([0x80, 0x3e, 1, 0x3e, 1, 0x75, 0xf9, 0xb8]);
    code.extend(first.to_le_bytes());
    code.push(0xb9);
    code.extend(count.to_le_bytes());
    far_call(&mut code, driver.selector);
    code.extend([0xc7, 0x06, 0x38, 0]);
    let vector_fixup = code.len();
    code.extend([0, 0, 0xc7, 0x06, 0x3a, 0, 0, 0xf0]);
    code.extend([
        0xb0, 8, 0xe6, 0xb0, 0xb0, 0x40, 0xe6, 0xb6, 0xe6, 0xb2, 0xfb,
    ]);
    let idle_address = 0xfe000 + code.len() as u32 + 1;
    code.extend([0xf4, 0xeb, 0xfd]);
    let interrupt = 0xe000 + code.len() as u16;
    code.extend([
        0x60, 0x1e, 0x06, 0x33, 0xc0, 0x8e, 0xd8, 0x8e, 0xc0, 0xb0, 0x40, 0xe6, 0xb6,
    ]);
    far_call(&mut code, driver.tick);
    far_call(&mut code, driver.status);
    code.extend([0x07, 0x1f, 0x61, 0xcf]);
    code[vector_fixup..vector_fixup + 2].copy_from_slice(&interrupt.to_le_bytes());
    ensure!(code.len() < 512, "WonderSwan direct bootstrap is too large");
    let mut result = bytes.to_vec();
    result[bootstrap..bootstrap + code.len()].copy_from_slice(&code);
    let reset = result.len() - 16;
    result[reset..reset + 5].copy_from_slice(&[0xea, 0, 0xe0, 0, 0xf0]);
    let checksum = result[..result.len() - 2]
        .iter()
        .fold(0_u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    let end = result.len();
    result[end - 2..].copy_from_slice(&checksum.to_le_bytes());
    Ok(PreparedWsTose {
        bytes: result,
        bootstrap: super::super::WsToseBootstrap::Cartridge,
        hardware: WsToseHardware::Color,
        ready_address: 0x3e00,
        ack_address: 0x3e01,
        wait_start,
        wait_end: wait_start + 7,
        timing: Some(super::super::WsToseTiming::DirectV1 { idle_address }),
    })
}

fn far_call(code: &mut Vec<u8>, offset: u16) {
    code.push(0x9a);
    code.extend(offset.to_le_bytes());
    code.extend(0x5000_u16.to_le_bytes());
}
