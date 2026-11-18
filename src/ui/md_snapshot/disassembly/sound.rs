use super::*;
use crate::debug::{DisassembledLine, DisassemblyCursor, InstructionBytes, Mnemonic};

pub(super) fn snapshot(
    emu: &Emulator,
    show: bool,
    target: Option<DisassemblyTarget>,
) -> Option<DisassemblyView> {
    if !show {
        return None;
    }
    let requested = target.and_then(DisassemblyTarget::as_cpu).filter(|target| {
        target.cpu_address <= 0xffff
            && target.storage_offset.is_none()
            && target.bank.is_none()
            && target.thumb.is_none()
    });
    let mut pc = requested.map_or_else(
        || emu.sound_cpu().regs().pc,
        |target| target.cpu_address as u16,
    );
    let cursor = DisassemblyCursor::Cpu(pc.into());
    let mut lines = Vec::with_capacity(LINES);
    for _ in 0..LINES {
        let Some(line) =
            crate::debug::z80_decode_known_instruction(|address| emu.peek_sound(address), pc)
        else {
            let mut mnemonic = Mnemonic::new();
            mnemonic.push_str("Unknown instruction bytes");
            lines.push(DisassembledLine {
                decode_status: None,
                address: pc.into(),
                storage_offset: None,
                bank: None,
                symbol: None,
                control_target: None,
                control_target_storage: None,
                control_target_bank: None,
                control_target_symbol: None,
                source: None,
                bytes: InstructionBytes::new(),
                mnemonic,
            });
            break;
        };
        pc = pc.wrapping_add(line.bytes.len() as u16);
        lines.push(line);
    }
    Some(DisassemblyView {
        cursor,
        mapping: None,
        is_navigation_target: requested.is_some(),
        is_static_target: false,
        location_symbol: None,
        lines,
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        hit_rom_breakpoint: None,
    })
}

#[cfg(test)]
mod tests {
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn unknown_sound_operands_never_decode_as_zero_and_odd_pc_is_valid() {
        for bytes in [
            &[0x3e][..],
            &[0xed][..],
            &[0xdd, 0xcb, 1][..],
            &[0x21, 0x34][..],
        ] {
            assert!(
                crate::debug::z80_decode_known_instruction(
                    |address| bytes.get(address as usize).copied(),
                    0
                )
                .is_none()
            );
        }
        let line = crate::debug::z80_decode_known_instruction(
            |address| [0x00, 0x3e, 0x5a].get(address as usize).copied(),
            1,
        )
        .unwrap();
        assert_eq!(line.bytes.as_slice(), [0x3e, 0x5a]);
        assert_eq!(line.mnemonic.as_str(), "LD A,$5A");
    }
}
