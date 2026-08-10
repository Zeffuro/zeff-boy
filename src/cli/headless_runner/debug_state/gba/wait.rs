use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::cpu::{
    ArmInstructionClass, DecodedInstruction, FetchedInstruction as GbaFetchedInstruction,
    ThumbInstructionClass,
};

pub(super) fn gba_last_swi_json(fetch: Option<GbaFetchedInstruction>) -> serde_json::Value {
    match gba_last_swi_function(fetch) {
        Some(function) => serde_json::json!({
            "present": true,
            "function": function,
            "function_hex": format!("{function:02X}"),
            "name": gba_swi_name(function),
            "wait_like": matches!(function, 0x02 | 0x04 | 0x05),
        }),
        None => serde_json::json!({ "present": false }),
    }
}

pub(in crate::cli::headless_runner) fn gba_wait_classification(
    emulator: &GbaEmulator,
) -> Option<&'static str> {
    match gba_last_swi_function(emulator.last_fetch()) {
        Some(0x02) => Some("gba-swi-halt-idle"),
        Some(0x04) => Some("gba-swi-intr-wait-idle"),
        Some(0x05) => Some("gba-swi-vblank-intr-wait-idle"),
        _ => gba_manual_vblank_poll_classification(emulator),
    }
}

fn gba_manual_vblank_poll_classification(emulator: &GbaEmulator) -> Option<&'static str> {
    const DISPSTAT: u32 = 0x0400_0004;

    let fetch = emulator.last_fetch()?;
    if !matches!(
        fetch.decoded,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadStoreHalfword,
        }
    ) {
        return None;
    }

    let regs = emulator.cpu_registers();
    (thumb_ldrh_immediate_address(fetch.raw, &regs) == Some(DISPSTAT))
        .then_some("gba-manual-vblank-poll")
}

fn thumb_ldrh_immediate_address(raw: u32, regs: &[u32; 16]) -> Option<u32> {
    let raw = u16::try_from(raw).ok()?;
    if raw & 0xF800 != 0x8800 {
        return None;
    }

    let offset = u32::from((raw >> 6) & 0x1F) * 2;
    let base_reg = usize::from((raw >> 3) & 0x07);
    Some(regs[base_reg].wrapping_add(offset))
}

fn gba_last_swi_function(fetch: Option<GbaFetchedInstruction>) -> Option<u32> {
    let fetch = fetch?;
    match fetch.decoded {
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SoftwareInterrupt,
            ..
        } => Some(fetch.raw & 0x00FF_FFFF),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::ConditionalBranchOrSwi,
        } if fetch.raw & 0xFF00 == 0xDF00 => Some(fetch.raw & 0xFF),
        _ => None,
    }
}

fn gba_swi_name(function: u32) -> &'static str {
    match function {
        0x01 => "RegisterRamReset",
        0x02 => "Halt",
        0x04 => "IntrWait",
        0x05 => "VBlankIntrWait",
        0x06 => "Div",
        0x07 => "DivArm",
        0x08 => "Sqrt",
        0x0B => "CpuSet",
        0x0C => "CpuFastSet",
        0x10 => "BitUnPack",
        0x11 => "LZ77UnCompReadNormalWrite8bit",
        0x12 => "LZ77UnCompReadNormalWrite16bit",
        0x13 => "HuffUnComp",
        0x14 => "RLUnCompReadNormalWrite8bit",
        0x15 => "RLUnCompReadNormalWrite16bit",
        0x16 => "Diff8bitUnFilterWrite8bit",
        0x17 => "Diff8bitUnFilterWrite16bit",
        0x18 => "Diff16bitUnFilter",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::thumb_ldrh_immediate_address;

    #[test]
    fn thumb_ldrh_immediate_address_decodes_dispstat_poll() {
        let mut regs = [0; 16];
        regs[2] = 0x0400_0004;

        assert_eq!(
            thumb_ldrh_immediate_address(0x8811, &regs),
            Some(0x0400_0004)
        );
    }

    #[test]
    fn thumb_ldrh_immediate_address_scales_halfword_offset() {
        let mut regs = [0; 16];
        regs[3] = 0x0400_0000;

        // LDRH r0, [r3, #4]
        assert_eq!(
            thumb_ldrh_immediate_address(0x8898, &regs),
            Some(0x0400_0004)
        );
    }

    #[test]
    fn thumb_ldrh_immediate_address_rejects_non_ldrh_halfword_ops() {
        let mut regs = [0; 16];
        regs[2] = 0x0400_0004;

        // STRH r1, [r2, #0]
        assert_eq!(thumb_ldrh_immediate_address(0x8011, &regs), None);
    }
}
