use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbChannelSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbChannelSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let profile = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Channel native profile is not recognized"))?;
    let mut code = vec![
        0xf3, 0x31, 0xc0, 0xc0, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07, 0xea, 0, 0x30, 0x3e, 1,
        0xea, 0, 0xc1,
    ];
    if profile.owarai {
        code.extend([0x3e, 0xf0, 0xea, 1, 0xc1]);
    }
    call(&mut code, profile.clear);
    bank(&mut code, 2, profile.shadow);
    call(&mut code, 0x4000);
    bank(&mut code, 7, profile.shadow);
    call(&mut code, profile.init);
    call(&mut code, profile.stop);
    code.extend([0xaf, 0xe0, 0xfd, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    bank(&mut code, 6, profile.shadow);
    code.extend([0x3e, profile.index as u8]);
    call(&mut code, profile.select);
    code.extend([0x3e, 0x91, 0xe0, 0x40, 0xaf, 0xe0, 0x0f]);
    if profile.owarai {
        call(&mut code, profile.rearm + 4);
        code.extend([0x3e, 4, 0xe0, 0x07]);
    }
    code.extend([0x3e, 5, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd]);
    let vblank = 0x150 + code.len() as u16;
    code.extend([0xf5, 0xc5, 0xd5, 0xe5]);
    if profile.owarai {
        call(&mut code, profile.rearm);
    } else {
        code.extend([
            0x21,
            profile.lock as u8,
            (profile.lock >> 8) as u8,
            0xcb,
            0x46,
            0x20,
            0x17,
        ]);
        let start = usize::from(profile.rearm);
        code.extend_from_slice(&bytes[start..start + 23]);
    }
    code.extend([0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    let end = 0x150 + code.len() as u32;
    anyhow::ensure!(
        end <= u32::from(profile.clear) && end <= 0x200,
        "Channel bootstrap exceeds reserved space"
    );
    for mapped in &song.mapped_spans {
        let start = mapped.effective_offset;
        let source_end = start + mapped.byte_len;
        for (reserved_start, reserved_end) in [(0x40, 0x43), (0x100, 0x103), (0x150, end)] {
            anyhow::ensure!(
                source_end <= reserved_start || start >= reserved_end,
                "Channel native wrapper overlaps admitted source bytes"
            );
        }
    }
    let mut output = bytes.to_vec();
    output[0x150..end as usize].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    output[0x40..0x43].copy_from_slice(&[0xc3, vblank as u8, (vblank >> 8) as u8]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: GbBankedTiming::Dmg,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffd,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    let [low, high] = address.to_le_bytes();
    code.extend([0xcd, low, high]);
}

fn bank(code: &mut Vec<u8>, value: u8, shadow: u16) {
    let [low, high] = shadow.to_le_bytes();
    code.extend([0x3e, value, 0xea, 0, 0x20, 0xea, low, high]);
}
