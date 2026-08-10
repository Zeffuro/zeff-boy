use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::cli::types::HeadlessOptions;

pub(super) fn print_gb_memory_dumps(emulator: &GbEmulator, opts: &HeadlessOptions) {
    for dump in &opts.memory_dumps {
        let start = dump.start_addr;
        let len = dump.len;
        println!("[mem] start={:04X} len={}", start, len);
        let mut offset = 0u16;
        while offset < len {
            let line_len = (len - offset).min(16);
            let addr = start.wrapping_add(offset);
            let bytes = (0..line_len)
                .map(|i| format!("{:02X}", emulator.peek_byte_raw(addr.wrapping_add(i))))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[mem] {:04X}: {}", addr, bytes);
            offset += line_len;
        }
    }
}
