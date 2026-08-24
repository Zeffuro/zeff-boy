use zeff_ws_core::emulator::Emulator as WsEmulator;

use crate::cli::types::HeadlessOptions;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WsPassFailTileStats {
    pub(super) pass_tiles: usize,
    pub(super) fail_tiles: usize,
}

pub(super) fn ws_background_screen_text(emulator: &WsEmulator) -> String {
    const SCREEN_BASE_PORT: u16 = 0x07;
    const MAP_WIDTH: usize = 32;
    const MAP_HEIGHT: usize = 32;

    let map_base_words = usize::from(emulator.io_peek8(SCREEN_BASE_PORT) & 0x0F) << 10;
    let mut text = String::new();
    for row in 0..MAP_HEIGHT {
        let mut line = String::with_capacity(MAP_WIDTH);
        for col in 0..MAP_WIDTH {
            let word_index = map_base_words + row * MAP_WIDTH + col;
            let byte_addr = (word_index * 2) as u32;
            let tile = u16::from_le_bytes([
                emulator.cpu_peek8(byte_addr),
                emulator.cpu_peek8(byte_addr.wrapping_add(1)),
            ]);
            line.push(ws_tile_text_char(tile));
        }

        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(trimmed);
        }
    }
    text
}

pub(super) fn ws_pass_fail_tile_stats(emulator: &WsEmulator) -> WsPassFailTileStats {
    const PASS_TILE: u16 = 5;
    const FAIL_TILE: u16 = 6;

    let mut stats = WsPassFailTileStats::default();
    for tile in ws_screen_1_tiles(emulator) {
        match tile & 0x01FF {
            PASS_TILE => stats.pass_tiles += 1,
            FAIL_TILE => stats.fail_tiles += 1,
            _ => {}
        }
    }
    stats
}

fn ws_screen_1_tiles(emulator: &WsEmulator) -> impl Iterator<Item = u16> + '_ {
    const SCREEN_BASE_PORT: u16 = 0x07;
    const MAP_WIDTH: usize = 32;
    const MAP_HEIGHT: usize = 32;

    let map_base_words = usize::from(emulator.io_peek8(SCREEN_BASE_PORT) & 0x0F) << 10;
    (0..MAP_HEIGHT).flat_map(move |row| {
        (0..MAP_WIDTH).map(move |col| {
            let word_index = map_base_words + row * MAP_WIDTH + col;
            let byte_addr = (word_index * 2) as u32;
            u16::from_le_bytes([
                emulator.cpu_peek8(byte_addr),
                emulator.cpu_peek8(byte_addr.wrapping_add(1)),
            ])
        })
    })
}

fn ws_tile_text_char(tile: u16) -> char {
    let byte = (tile & 0x00FF) as u8;
    if byte == 0 {
        ' '
    } else if byte.is_ascii_graphic() || byte == b' ' {
        char::from(byte)
    } else {
        '.'
    }
}

pub(super) fn compact_ws_text(text: &str) -> String {
    let mut compact = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    const MAX_CHARS: usize = 1200;
    if compact.chars().count() > MAX_CHARS {
        compact = compact.chars().take(MAX_CHARS).collect();
        compact.push_str("\n...");
    }
    compact
}

pub(super) fn print_ws_memory_dumps(emulator: &WsEmulator, opts: &HeadlessOptions) {
    for dump in &opts.memory_dumps {
        let start = u32::from(dump.start_addr);
        let len = u32::from(dump.len);
        println!("[mem] start={start:05X} len={len}");
        let mut offset = 0u32;
        while offset < len {
            let line_len = (len - offset).min(16);
            let addr = start.wrapping_add(offset);
            let bytes = (0..line_len)
                .map(|i| format!("{:02X}", emulator.cpu_peek8(addr.wrapping_add(i))))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[mem] {addr:05X}: {bytes}");
            offset += line_len;
        }
    }
}

pub(super) fn ws_wait_classification(emulator: &WsEmulator) -> Option<&'static str> {
    if emulator.cpu_state() == zeff_ws_core::hardware::cpu::CpuState::Halted
        && emulator.io_peek8(0xB2) != 0
    {
        Some("ws-halt-idle")
    } else if ws_framebuffer_has_visible_content(emulator.framebuffer()) {
        Some("ws-static-visible-frame")
    } else {
        None
    }
}

fn ws_framebuffer_has_visible_content(framebuffer: &[u8]) -> bool {
    let mut chunks = framebuffer.as_chunks::<4>().0.iter();
    let Some(first) = chunks.next() else {
        return false;
    };
    chunks.any(|pixel| pixel[..3] != first[..3])
}

pub(super) fn ws_progress_marker(emulator: &WsEmulator) -> Option<u64> {
    let fetched = emulator.last_fetch()?;
    if fetched.pc != emulator.cpu_pc() || !ws_string_opcode(fetched.opcode) {
        return None;
    }

    let regs = emulator.cpu_registers();
    let segments = emulator.cpu_segments();
    let mut marker = 0xCBF2_9CE4_8422_2325u64;
    for value in [
        fetched.pc as u64,
        fetched.opcode as u64,
        regs[0] as u64,
        regs[1] as u64,
        regs[2] as u64,
        regs[6] as u64,
        regs[7] as u64,
        segments[0] as u64,
        segments[3] as u64,
        emulator.cpu_flags() as u64,
    ] {
        marker ^= value;
        marker = marker.wrapping_mul(0x0000_0100_0000_01B3);
    }
    Some(marker)
}

fn ws_string_opcode(opcode: u8) -> bool {
    matches!(opcode, 0x6C..=0x6F | 0xA4..=0xA7 | 0xAA..=0xAF)
}
