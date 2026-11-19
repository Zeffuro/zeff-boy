use super::*;
use zeff_emu_common::debug::{TraceBoundary, TraceObservation};

fn z80(bytes: &[u8], pc: u16) -> InstructionTraceRecord {
    InstructionTraceRecord::new(TraceExecMode::Z80, u32::from(pc), None, 3, 41, bytes)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn z80_trace_rejects_missing_operands_without_inventing_zero() {
    for pc in [0x2000, 0xffff] {
        let missing = decode_entry(&z80(&[0x3e], pc));
        assert_eq!(missing.bytes, "3E");
        assert!(missing.mnemonic.starts_with("Unknown assembly"));
        let zero = decode_entry(&z80(&[0x3e, 0], pc));
        assert_eq!(zero.bytes, "3E 00");
        assert_eq!(zero.mnemonic, "LD A,$00");
        for bytes in [
            &[0x21, 0][..],
            &[0xc3, 0],
            &[0xed, 0x43, 0],
            &[0xdd, 0x36, 0],
        ] {
            let decoded = decode_entry(&z80(bytes, pc));
            assert_eq!(decoded.bytes.split_whitespace().count(), bytes.len());
            assert!(decoded.mnemonic.starts_with("Unknown assembly"));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn z80_trace_accepts_complete_short_forms_and_rejects_unsupported_prefix_history() {
    for pc in [0x2000, 0xffff] {
        for (bytes, mnemonic) in [
            (&[0][..], "NOP"),
            (&[0x76], "HALT"),
            (&[0x3e, 0], "LD A,$00"),
            (&[0x21, 0, 0], "LD HL,$0000"),
            (&[0xcb, 0x46], "BIT 0,(HL)"),
            (&[0xed, 0x56], "IM 1"),
            (&[0xed, 0xb0], "LDIR"),
            (&[0xed, 0x43, 0, 0], "LD ($0000),BC"),
            (&[0xdd, 0x3e, 0], "LD A,$00"),
            (&[0xdd, 0x7e, 0], "LD A,(IX+$00)"),
            (&[0xfd, 0xcb, 0, 0x46], "BIT 0,(IY+$00)"),
        ] {
            let decoded = decode_entry(&z80(bytes, pc));
            assert_eq!(decoded.mnemonic, mnemonic, "{bytes:02x?}");
            assert_eq!(decoded.bytes.split_whitespace().count(), bytes.len());
        }
        for bytes in [
            &[0xdd, 0xdd][..],
            &[0xdd, 0xfd, 0x3e, 0x5a],
            &[0xdd, 0xed, 0x43, 0, 0x40],
            &[0xdd, 0xfd, 0xdd, 0xfd, 0xcb, 0, 0x46],
            &[0x3e, 0, 0x76],
        ] {
            let entry = z80(bytes, pc);
            let decoded = decode_entry(&entry);
            assert!(decoded.mnemonic.starts_with("Unknown assembly"));
            assert_eq!(decoded.bytes.split_whitespace().count(), bytes.len());
            let captured = bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(decoded.bytes, captured);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_suffix_and_interruption_keep_committed_effects_and_truthful_time_labels() {
    for start in [
        TraceObservationStart::Instruction,
        TraceObservationStart::Suffix,
    ] {
        for end in [
            TraceObservationEnd::Retired,
            TraceObservationEnd::Fault,
            TraceObservationEnd::Reset,
            TraceObservationEnd::Disabled,
        ] {
            let mut entry = z80(&[0x3e, 0x5a], 0x2000);
            entry.observation = TraceObservation {
                start,
                end,
                ..Default::default()
            };
            entry.boundary = Some(TraceBoundary {
                kind: TraceBoundaryKind::Instruction,
                cpu_clocks: 7,
                final_exception: None,
                machine_fault: false,
            });
            entry.push_write(TraceWrite {
                address: 0x100,
                old_value: Some(0x5a),
                new_value: 0x5a,
                width: TraceWriteWidth::Byte,
                kind: TraceWriteKind::Memory,
            });
            let decoded = decode_entry(&entry);
            assert_eq!(decoded.bytes, "3E 5A");
            let changes = format_changes(&entry);
            assert!(changes.contains("[100]=5A>5A"));
            if start == TraceObservationStart::Suffix {
                assert_eq!(format_cycle(&entry), "Observation start 41");
                assert!(changes.contains("Observation start 41; unknown instruction prefix"));
                assert!(changes.contains("Observation span 7 CPU clocks"));
                assert!(decoded.mnemonic.starts_with("Unknown assembly"));
                assert_eq!(entry.instruction_overflow, 0);
            } else {
                assert_eq!(format_cycle(&entry), "41");
                assert!(!changes.contains("unknown instruction prefix"));
            }
            if end == TraceObservationEnd::Retired {
                if start == TraceObservationStart::Instruction {
                    assert_eq!(decoded.mnemonic, "LD A,$5A");
                    assert_eq!(changes, "Instruction; 7 CPU clocks [100]=5A>5A");
                }
            } else {
                assert!(decoded.mnemonic.starts_with("Unknown assembly"));
                assert!(changes.contains(&format!(
                    "Observation interrupted: {}",
                    observation_end_label(end)
                )));
                assert!(changes.contains("Observation span 7 CPU clocks"));
            }
        }
    }
    let mut empty = z80(&[], 0x2000);
    empty.observation.start = TraceObservationStart::Suffix;
    empty.push_write(TraceWrite {
        address: 0x100,
        new_value: 0x5a,
        ..Default::default()
    });
    let decoded = decode_entry(&empty);
    assert_eq!(decoded.bytes, "");
    assert!(decoded.mnemonic.starts_with("Unknown assembly"));
    let changes = format_changes(&empty);
    assert!(changes.contains("Observation start 41; unknown instruction prefix"));
    assert!(changes.contains("[100]=??>5A"));
    assert!(!changes.contains("span"));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_byte_loss_is_known_and_saturation_is_only_a_lower_bound() {
    for start in [
        TraceObservationStart::Instruction,
        TraceObservationStart::Suffix,
    ] {
        for omitted in [1, usize::from(u16::MAX), usize::from(u16::MAX) + 1] {
            let mut bytes = vec![0xdd; 16 + omitted];
            bytes[0] = 0x3e;
            bytes[1] = 0;
            let mut entry = z80(&bytes, 0xffff);
            entry.observation.start = start;
            let decoded = decode_entry(&entry);
            assert_eq!(
                decoded.bytes,
                "3E 00 DD DD DD DD DD DD DD DD DD DD DD DD DD DD"
            );
            assert!(decoded.mnemonic.starts_with("Truncated assembly"));
            let changes = format_changes(&entry);
            if omitted == 1 {
                assert!(changes.contains("+1 instruction bytes"));
                assert!(!changes.contains("at least"));
            } else {
                assert!(changes.contains("at least 65535 instruction bytes (saturated)"));
            }
            assert_eq!(
                changes.contains("unknown instruction prefix"),
                start == TraceObservationStart::Suffix
            );
        }
    }
    let mut fault = z80(&[0x3e], 0xffff);
    fault.boundary = Some(TraceBoundary {
        kind: TraceBoundaryKind::Fault,
        cpu_clocks: 4,
        final_exception: None,
        machine_fault: true,
    });
    let decoded = decode_entry(&fault);
    assert_eq!(decoded.bytes, "3E");
    assert_eq!(decoded.mnemonic, "Boundary failed; partial bytes");
    assert!(format_changes(&fault).contains("4 CPU clocks; machine fault"));
}
