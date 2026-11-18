use crate::debug::{DisassembledLine, DisassemblyView, InstructionBytes, Mnemonic};
use zeff_m68k::disassembly::{DecodeStatus, decode_unmapped};

pub(super) fn snapshot(rom: &[u8], offset: u64) -> DisassemblyView {
    let mut view = DisassemblyView::unavailable_rom(offset);
    let Ok(mut index) = usize::try_from(offset) else {
        return view;
    };
    for _ in 0..super::LINES {
        let Some(bytes) = rom.get(index..).filter(|bytes| !bytes.is_empty()) else {
            break;
        };
        let Ok(address) = u32::try_from(index) else {
            break;
        };
        let instruction = decode_unmapped(bytes);
        let count = instruction.bytes.len();
        let stopped = matches!(
            instruction.status,
            DecodeStatus::Unavailable | DecodeStatus::Unaligned
        );
        view.lines.push(DisassembledLine {
            address,
            decode_status: Some(instruction.status),
            storage_offset: Some(index as u64),
            bank: None,
            symbol: None,
            control_target: None,
            control_target_storage: None,
            control_target_bank: None,
            control_target_symbol: None,
            source: None,
            bytes: InstructionBytes::from_iter(instruction.bytes),
            mnemonic: Mnemonic::try_from(instruction.text.as_str())
                .unwrap_or_else(|_| Mnemonic::try_from("<instruction text too long>").unwrap()),
        });
        if stopped || count == 0 {
            break;
        }
        let Some(next) = index.checked_add(count) else {
            break;
        };
        index = next;
    }
    view
}

#[cfg(test)]
mod mapper_tests;
#[cfg(test)]
mod tests;
