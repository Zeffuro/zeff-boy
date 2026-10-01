use super::*;

pub(in crate::ws_tose) fn prepare_rom(
    bytes: &[u8],
    song: &WsToseSong,
    cancel: &AtomicBool,
) -> Result<PreparedWsTose> {
    let profile = checked_profile(bytes, song, cancel)?;
    let driver = profile.driver;
    let (first, _, entry) = selector(bytes, profile, song.index)
        .map_err(|_| anyhow::anyhow!("invalid WonderSwan volume selector"))?;
    let bootstrap = bytes.len() - 0x2000;
    ensure!(
        driver.segment == 0xe000
            && bytes.len() == 0x400000
            && driver.slots == 0x2000
            && song.mapped_spans.iter().all(|span| {
                let start = span.effective_offset as usize;
                let end = start + span.byte_len as usize;
                end <= bootstrap || start >= bootstrap + 512
            }),
        "WonderSwan volume driver overlaps bootstrap storage"
    );
    let mut code = vec![
        0xfa, 0xfc, 0x33, 0xc0, 0x8e, 0xd0, 0xbc, 0xf0, 0x3d, 0x8e, 0xd8, 0x8e, 0xc0, 0x33, 0xff,
        0xb9, 0, 0x80, 0xf3, 0xab, 0xe6, 0xb2, 0xb0, 3, 0xe6, 0xc0,
    ];
    let color = profile
        .control_span(profile.color_setup, 4)
        .effective_offset as usize;
    ensure!(
        bytes.get(color..color + 4) == Some(&[0xb0, 0xea, 0xe6, 0x60]),
        "WonderSwan Color setup differs from its source"
    );
    code.extend_from_slice(&bytes[color..color + 4]);
    far_call(&mut code, driver.init);
    let setup = profile
        .control_span(profile.volume_setup, 5)
        .effective_offset as usize;
    ensure!(
        bytes.get(setup..setup + 5) == Some(&[0xc6, 0x06, 0x4a, 0, 0x3f]),
        "WonderSwan master-volume setup differs from its source"
    );
    code.extend_from_slice(&bytes[setup..setup + 5]);
    code.extend([0xc6, 0x06, 0, 0x3e, 1]);
    let wait_start = 0xfe000 + code.len() as u32;
    code.extend([0x80, 0x3e, 1, 0x3e, 1, 0x75, 0xf9, 0xb8]);
    code.extend(first.to_le_bytes());
    far_call(&mut code, entry);
    code.extend([0xc7, 0x06, 0x1c, 0]);
    code.extend(profile.interrupt.to_le_bytes());
    code.extend([
        0xc7, 0x06, 0x1e, 0, 0, 0xf0, 0xb0, 0, 0xe6, 0xb0, 0xb8, 0xa0, 0, 0xe7, 0xa4, 0xb0, 0x80,
        0xe6, 0xb6, 0xe6, 0xb2, 0xb0, 3, 0xe6, 0xa2, 0xfb,
    ]);
    let idle_address = 0xfe000 + code.len() as u32 + 1;
    code.extend([0xf4, 0xeb, 0xfd]);
    ensure!(code.len() < 512, "WonderSwan volume bootstrap is too large");
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
        timing: Some(super::super::WsToseTiming::VolumeV1 { idle_address }),
    })
}

fn far_call(code: &mut Vec<u8>, offset: u16) {
    code.push(0x9a);
    code.extend(offset.to_le_bytes());
    code.extend(0xe000_u16.to_le_bytes());
}
