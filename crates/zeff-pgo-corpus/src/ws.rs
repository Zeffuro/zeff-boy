//! Guest-driven WonderSwan PGO fixtures.
//!
//! The V30 program configures the display and PSG itself, fills RAM with a
//! changing byte pattern, uses string moves, and then keeps changing tile,
//! sprite, waveform, and scroll data. It deliberately avoids host-side RAM or
//! I/O setup so the release frontend trains the normal CPU/bus paths.

pub(super) fn rom(color: bool) -> Vec<u8> {
    let mut rom = vec![0x90; 0x10000];
    let mut code = Vec::new();

    // Real-mode setup: DS/ES address internal RAM, while CS:F000 remains the
    // mapped cartridge window. Interrupts stay disabled because this fixture
    // has no guest interrupt vectors.
    code.extend_from_slice(&[0xFA, 0x31, 0xC0, 0x8E, 0xD8, 0x8E, 0xC0]);

    // Enable both tile layers and sprites; select map/sprite bases, scroll,
    // LCD, and (on Color) the 16-colour VDP mode. These are the same register
    // families exercised by the former profile_cores host setup.
    out_imm8(&mut code, 0x00, 0x07);
    out_imm8(&mut code, 0x04, if color { 0x30 } else { 0x10 });
    out_imm8(&mut code, 0x05, 0x00);
    out_imm8(&mut code, 0x06, 64);
    out_imm8(&mut code, 0x07, 0x21);
    out_imm8(&mut code, 0x10, 13);
    out_imm8(&mut code, 0x11, 29);
    out_imm8(&mut code, 0x12, 47);
    out_imm8(&mut code, 0x13, 71);
    out_imm8(&mut code, 0x14, 0x01);
    if color {
        out_imm8(&mut code, 0x60, 0xC0);
    } else {
        // Give the mono tile pens distinct shades; leaving palette registers
        // at zero collapses every tile pixel to the same default shade.
        out_imm8(&mut code, 0x20, 0x10);
        out_imm8(&mut code, 0x21, 0x32);
    }

    // Two independently advancing wave channels, both routed left/right.
    // RAM is populated below, so their waveform nibbles are non-constant.
    out_imm8(&mut code, 0x80, 0x00);
    out_imm8(&mut code, 0x81, 0x07);
    out_imm8(&mut code, 0x82, 0x40);
    out_imm8(&mut code, 0x83, 0x08);
    out_imm8(&mut code, 0x88, 0xFF);
    out_imm8(&mut code, 0x89, 0xEE);
    out_imm8(&mut code, 0x90, 0x03);

    let fill_count = if color { 0xFFFF } else { 0x3FFF };
    // Fill the available RAM with an arithmetic pattern. The explicit
    // LOOP branch is intentional: it trains branch/ALU/RAM traffic rather
    // than relying on an emulator-side bulk operation.
    code.extend_from_slice(&[
        0xBF,
        0x00,
        0x00,
        0xB9,
        fill_count as u8,
        (fill_count >> 8) as u8,
        0xB0,
        0x5A,
    ]);
    let fill_loop = code.len();
    code.extend_from_slice(&[0xAA, 0x04, 0x17, 0xE2, 0x00]);
    let fill_branch = code.len() - 1;
    patch_rel8(&mut code, fill_branch, fill_loop);

    // Copy a populated tile region to the sprite-table range with MOVSB.
    // This exercises the V30 string read/write implementation in addition to
    // the byte-at-a-time arithmetic loop.
    let copy_source = if color { 0x2000_u16 } else { 0x0800 };
    let copy_destination = if color { 0x6000_u16 } else { 0x2000 };
    code.extend_from_slice(&[
        0xBE,
        copy_source as u8,
        (copy_source >> 8) as u8,
        0xBF,
        copy_destination as u8,
        (copy_destination >> 8) as u8,
        0xB9,
        0x00,
        0x10,
    ]);
    let copy_loop = code.len();
    code.extend_from_slice(&[0xA4, 0xE2, 0x00]);
    let copy_branch = code.len() - 1;
    patch_rel8(&mut code, copy_branch, copy_loop);

    // Sustained guest work. It reads/modifies/writes a bounded RAM window,
    // updates the waveform area used by the APU, and varies scroll every pass.
    let work_base = if color { 0x2000_u16 } else { 0x0000 };
    let work_step = if color { 0x40 } else { 0x10 };
    let work_limit = if color { 0x20 } else { 0x40 };
    code.extend_from_slice(&[0xBB, work_base as u8, (work_base >> 8) as u8]);
    let work_loop = code.len();
    code.extend_from_slice(&[
        0x8A, 0x07, // mov al,[bx]
        0x04, 0x13, // add al,13h
        0x32, 0x07, // xor al,[bx]
        0x88, 0x07, // mov [bx],al
        0xE6, 0x10, // out 10h,al (background scroll)
        0xE6, 0x11, // out 11h,al (background scroll)
        0xFE, 0xC3, // inc bl; high byte advances only in the bounded step below
        0x80, 0xFB, 0x00, // cmp bl,0
        0x75, 0x00, // jnz work_loop
    ]);
    let work_branch = code.len() - 1;
    patch_rel8(&mut code, work_branch, work_loop);
    // High byte advances through a small, explicit RAM set before reset. This
    // keeps mono's 16 KiB and Color's 64 KiB guest accesses in bounds.
    code.extend_from_slice(&[
        0x80,
        0xC7,
        work_step,
        0x80,
        0xFF,
        work_limit,
        0x75,
        0x00,
        0xBB,
        work_base as u8,
        (work_base >> 8) as u8,
        0xEB,
        0x00,
    ]);
    let high_byte_branch = code.len() - 6;
    let restart_branch = code.len() - 1;
    patch_rel8(&mut code, high_byte_branch, work_loop);
    patch_rel8(&mut code, restart_branch, work_loop);

    rom[..code.len()].copy_from_slice(&code);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 1] = u8::from(color);
    rom[footer + 4] = 0x01;
    let checksum: u16 = rom[..rom.len() - 2]
        .iter()
        .fold(0u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn out_imm8(code: &mut Vec<u8>, port: u8, value: u8) {
    code.extend_from_slice(&[0xB0, value, 0xE6, port]);
}

fn patch_rel8(code: &mut [u8], displacement_at: usize, target: usize) {
    let after = displacement_at + 1;
    let displacement = isize::try_from(target).unwrap() - isize::try_from(after).unwrap();
    code[displacement_at] = i8::try_from(displacement)
        .expect("WonderSwan guest branch must remain within rel8 range")
        as u8;
}
