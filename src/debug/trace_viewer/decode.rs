use super::*;

pub(super) struct DecodedTrace {
    pub(super) bytes: String,
    pub(super) mnemonic: String,
}

pub(super) fn trace_cpu_address(entry: &InstructionTraceRecord) -> u32 {
    if entry.mode == TraceExecMode::M68000 {
        entry.pc & 0xff_ffff
    } else {
        entry.pc
    }
}

pub(super) fn decode_entry(entry: &InstructionTraceRecord) -> DecodedTrace {
    let captured = || {
        entry
            .instruction_bytes()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    if entry
        .boundary
        .is_some_and(|boundary| boundary.kind == TraceBoundaryKind::Fault)
    {
        return DecodedTrace {
            bytes: captured(),
            mnemonic: "Boundary failed; partial bytes".into(),
        };
    }
    let unavailable = if entry.instruction_overflow != 0 {
        Some("Truncated assembly; instruction bytes omitted".to_owned())
    } else if entry.observation.start == TraceObservationStart::Suffix {
        Some("Unknown assembly; instruction prefix unobserved".to_owned())
    } else if entry.observation.end != TraceObservationEnd::Retired {
        Some(format!(
            "Unknown assembly; observation interrupted ({})",
            observation_end_label(entry.observation.end)
        ))
    } else {
        None
    };
    if let Some(mnemonic) = unavailable {
        return DecodedTrace {
            bytes: captured(),
            mnemonic,
        };
    }
    if entry.instruction_len == 0 {
        return DecodedTrace {
            bytes: "--".to_string(),
            mnemonic: entry
                .boundary
                .map(|boundary| boundary_label(boundary.kind).to_owned())
                .unwrap_or_else(|| {
                    entry
                        .event
                        .map_or_else(|| "Idle".to_string(), |event| event.label().to_string())
                }),
        };
    }
    let bytes = &entry.instruction[..usize::from(entry.instruction_len)];
    let line = match entry.mode {
        TraceExecMode::Sm83 => super::super::disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::Mos6502 => super::super::nes_disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::HuC6280 => super::super::huc6280_disassemble_around(
            |address| trace_read16(bytes, entry.pc as u16, address),
            entry.pc as u16,
            0,
            1,
        ),
        TraceExecMode::Z80 => return decode_z80(entry.pc as u16, bytes, captured()),
        TraceExecMode::Arm | TraceExecMode::Thumb => super::super::gba_disassemble_around(
            |address| trace_read32(bytes, entry.pc, address),
            entry.pc,
            entry.mode == TraceExecMode::Thumb,
            0,
            1,
        ),
        TraceExecMode::V30 => super::super::v30_disassemble_around(
            |address| trace_read32(bytes, entry.pc, address),
            entry.pc,
            0,
            1,
        ),
        TraceExecMode::M68000 => super::super::m68k_disassemble_forward(
            |address| {
                bytes
                    .get((address.wrapping_sub(entry.pc) & 0xff_ffff) as usize)
                    .copied()
            },
            entry.pc,
            1,
        ),
    }
    .into_iter()
    .next();
    if let Some(line) = line {
        DecodedTrace {
            bytes: if entry.mode == TraceExecMode::M68000 {
                captured()
            } else {
                line.bytes
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            },
            mnemonic: line.mnemonic.to_string(),
        }
    } else {
        DecodedTrace {
            bytes: format!("{:02X}", bytes[0]),
            mnemonic: format!("DB ${:02X}", bytes[0]),
        }
    }
}

fn trace_read16(bytes: &[u8], pc: u16, address: u16) -> u8 {
    bytes
        .get(usize::from(address.wrapping_sub(pc)))
        .copied()
        .unwrap_or(0)
}

fn trace_read32(bytes: &[u8], pc: u32, address: u32) -> u8 {
    bytes
        .get(address.wrapping_sub(pc) as usize)
        .copied()
        .unwrap_or(0)
}

fn decode_z80(pc: u16, bytes: &[u8], captured: String) -> DecodedTrace {
    let missing = std::cell::Cell::new(false);
    let line = super::super::disassembler::z80_decode_instruction(
        |address| match bytes.get(usize::from(address.wrapping_sub(pc))) {
            Some(value) => *value,
            None => {
                missing.set(true);
                0
            }
        },
        pc,
    );
    let mnemonic =
        if missing.get() || line.bytes.as_slice() != bytes || line.mnemonic.starts_with("PREFIX") {
            "Unknown assembly; incomplete or unsupported instruction bytes".to_owned()
        } else {
            line.mnemonic.to_string()
        };
    DecodedTrace {
        bytes: captured,
        mnemonic,
    }
}
