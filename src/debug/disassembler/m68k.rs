use super::{DisassembledLine, InstructionBytes, Mnemonic};
use zeff_m68k::disassembly::{DecodeStatus, decode};

pub(crate) const ADDRESS_MASK: u32 = 0x00ff_ffff;

pub(crate) fn disassemble_forward(
    read: impl Fn(u32) -> Option<u8>,
    pc: u32,
    count: usize,
) -> Vec<DisassembledLine> {
    let mut lines = Vec::with_capacity(count);
    let mut address = pc;
    for _ in 0..count {
        let instruction = decode(address, |address| read(address & ADDRESS_MASK));
        let stopped = matches!(
            instruction.status,
            DecodeStatus::Unavailable | DecodeStatus::Unaligned
        );
        let next = address.wrapping_add(instruction.bytes.len() as u32);
        lines.push(DisassembledLine {
            address: address & ADDRESS_MASK,
            storage_offset: None,
            bank: None,
            symbol: None,
            control_target: instruction
                .control_target
                .map(|target| target & ADDRESS_MASK),
            control_target_storage: None,
            control_target_bank: None,
            control_target_symbol: None,
            source: None,
            bytes: InstructionBytes::from_iter(instruction.bytes),
            mnemonic: Mnemonic::try_from(instruction.text.as_str())
                .unwrap_or_else(|_| mn!("<instruction text too long>")),
        });
        if stopped {
            break;
        }
        address = next;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn forward_view_keeps_extension_bytes_targets_and_text() {
        let bytes = [
            0x20, 0x3c, 0x12, 0x34, 0x56, 0x78, 0x4e, 0xf9, 0, 0xff, 0, 0x10, 0x60, 0xff,
        ];
        let lines = disassemble_forward(
            |a| bytes.get(a.wrapping_sub(0x10000) as usize).copied(),
            0x10000,
            4,
        );
        assert_eq!(
            lines.iter().map(|l| l.address).collect::<Vec<_>>(),
            [0x10000, 0x10006, 0x1000c, 0x1000e]
        );
        assert_eq!(lines[0].bytes.as_slice(), &bytes[..6]);
        assert_eq!(lines[0].mnemonic.as_str(), "MOVE.L #$12345678,D0");
        assert_eq!(lines[1].control_target, Some(0xff0010));
        assert_eq!(lines[2].control_target, Some(0x1000d));
        assert_eq!(lines[3].mnemonic.as_str(), "<unavailable>");
        let bytes = [0x48, 0xf9, 0x55, 0x55, 0x12, 0x34, 0x56, 0x78];
        let line = disassemble_forward(|a| bytes.get(a as usize).copied(), 0, 1)
            .pop()
            .unwrap();
        assert!(line.mnemonic.len() > 32);
        assert!(!line.mnemonic.contains("too long"));
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn holes_and_alignment_stop_without_fabricating_bytes_or_predecessors() {
        let calls = Cell::new(0);
        let lines = disassemble_forward(
            |a| {
                calls.set(calls.get() + 1);
                (a == 0xc00000).then_some(0x20)
            },
            0xc00000,
            26,
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].bytes.as_slice(), &[0x20]);
        assert_eq!(lines[0].mnemonic.as_str(), "<unavailable>");
        let lines = disassemble_forward(|_| panic!("unaligned view read"), 0xff0001, 26);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].mnemonic.as_str(), "<unaligned>");
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn physical_reads_and_lines_wrap_without_truncating_cpu_operands() {
        let bytes = [0x60, 0x02, 0x4e, 0x71, 0x4e, 0x71];
        let lines = disassemble_forward(
            |a| {
                bytes
                    .get((a.wrapping_sub(0xfffffe) & ADDRESS_MASK) as usize)
                    .copied()
            },
            0xfffffe,
            3,
        );
        assert_eq!(
            lines.iter().map(|l| l.address).collect::<Vec<_>>(),
            [0xfffffe, 0, 2]
        );
        assert_eq!(lines[0].control_target, Some(2));
        assert_eq!(lines[0].mnemonic.as_str(), "BRA.S $01000002");
        let upper = disassemble_forward(
            |a| {
                bytes
                    .get((a.wrapping_sub(0xfffffe) & ADDRESS_MASK) as usize)
                    .copied()
            },
            0xabfffffe,
            1,
        );
        assert_eq!(upper[0].address, 0xfffffe);
        assert_eq!(upper[0].control_target, Some(2));
        assert_eq!(upper[0].mnemonic.as_str(), "BRA.S $AC000002");
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn real_vdp_peek_holes_stop_without_reading_ports_or_mutating_state() {
        let mut rom = vec![0; 0x204];
        rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
        rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
        rom[0x200..].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        let emu = zeff_md_core::Emulator::new(rom, zeff_md_core::LoadConfig::default()).unwrap();
        let state = emu.encode_state().unwrap();
        let lines = disassemble_forward(|a| emu.peek(a), 0xc00000, 26);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].bytes.is_empty());
        assert_eq!(lines[0].mnemonic.as_str(), "<unavailable>");
        assert_eq!(emu.encode_state().unwrap(), state);
    }
}
