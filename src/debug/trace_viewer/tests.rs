use super::*;

#[test]
fn huc6280_trace_entries_decode_and_name_mapping_registers() {
    let mut entry = InstructionTraceRecord::new(
        TraceExecMode::HuC6280,
        0xE000,
        Some(0x20_000),
        3,
        45,
        &[0x73, 0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A],
    );
    entry.bank = Some(0x10);

    let decoded = decode_entry(&entry);
    assert_eq!(decoded.bytes, "73 34 12 78 56 BC 9A");
    assert_eq!(decoded.mnemonic, "TII $1234,$5678,$9ABC");
    assert_eq!(register_name(TraceExecMode::HuC6280, 6), "MPR0");
    assert_eq!(register_name(TraceExecMode::HuC6280, 14), "SPD");
    assert_eq!(format_pc(&entry), "E000");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_trace_presents_consumed_bytes_fault_prefix_and_unknown_bus_values() {
    use zeff_emu_common::debug::{TraceBoundary, TraceBoundaryKind};
    let mut entry = InstructionTraceRecord::new(
        TraceExecMode::M68000,
        0x200,
        None,
        0,
        12,
        &[0x23, 0xfc, 0x12, 0x34, 0x56, 0x78, 0, 0xff, 0, 0x40],
    );
    let decoded = decode_entry(&entry);
    assert_eq!(decoded.bytes, "23 FC 12 34 56 78 00 FF 00 40");
    assert!(
        decoded.mnemonic.starts_with("MOVE.L"),
        "{}",
        decoded.mnemonic
    );
    let write = TraceWrite {
        address: 0xff0040,
        old_value: None,
        new_value: 0x1234,
        width: TraceWriteWidth::Halfword,
        kind: TraceWriteKind::Bus,
    };
    assert_eq!(format_write(&write), "BUS[FF0040]=??>1234");
    entry.push_write(write);
    assert!(entry_matches(&entry, "write:FF0041", &Default::default()));
    assert!(!entry_matches(&entry, "write:FF0042", &Default::default()));
    assert_eq!(
        format_write(&TraceWrite {
            old_value: Some(0xabcd),
            kind: TraceWriteKind::Memory,
            ..write
        }),
        "[FF0040]=ABCD>1234"
    );
    entry.set_instruction(&[0x23, 0xfc, 0x12, 0x34]);
    entry.boundary = Some(TraceBoundary {
        kind: TraceBoundaryKind::Fault,
        cpu_clocks: 18,
        final_exception: None,
        machine_fault: true,
    });
    let decoded = decode_entry(&entry);
    assert_eq!(decoded.bytes, "23 FC 12 34");
    assert_eq!(decoded.mnemonic, "Boundary failed; partial bytes");
    assert!(format_changes(&entry).contains("18 CPU clocks; machine fault"));
    assert_eq!(register_name(TraceExecMode::M68000, 16), "SSP");
    assert_eq!(register_name(TraceExecMode::M68000, 18), "SR");
    assert_eq!(format_rom(entry.physical_rom_offset), "--");
    entry.pc = 0xabfffffe;
    entry.boundary = None;
    entry.set_instruction(&[0x60, 2]);
    assert!(decode_entry(&entry).mnemonic.contains("AC000002"));
    assert_eq!(trace_cpu_address(&entry), 0xfffffe);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_trace_labels_unknown_frame_and_ignores_main_symbol_namespace() {
    use crate::symbols::*;
    use crate::ui::TraceTarget;
    let mut symbols = SymbolSession::default();
    symbols.store.insert(SymbolRecord {
        id: SymbolId(0),
        name: "MainOnly".into(),
        location: SymbolLocation {
            cpu: Some(CpuLocation {
                space: AddressSpaceId(0),
                address: 0,
            }),
            storage: None,
            bank: None,
            exec_mode: ExecMode::M68000,
        },
        value: None,
        size: Some(1),
        kind: SymbolKind::Function,
        scope: SymbolScope::Global,
        provenance: Provenance {
            kind: ProvenanceKind::Build,
            source: None,
        },
        confidence: Confidence::Exact,
        comment: None,
    });
    let mut row = InstructionTraceRecord::new(TraceExecMode::Z80, 0, None, 0, 0, &[0x3e, 0x5a]);
    assert_eq!(
        trace_symbol_for_target(TraceTarget::Primary, &symbols, &row).as_deref(),
        Some("MainOnly")
    );
    assert!(trace_symbol_for_target(TraceTarget::Sound, &symbols, &row).is_none());
    assert!(!entry_matches_target(
        TraceTarget::Sound,
        &row,
        "mainonly",
        &symbols
    ));
    assert!(entry_matches_target(
        TraceTarget::Sound,
        &row,
        "pc:0000",
        &symbols
    ));
    row.observation.frame_knowledge = TraceFrameKnowledge::Unknown;
    assert_eq!(trace_frame_label(&row), "Unknown");
    assert_eq!(register_name(row.mode, 10), "SP");
    assert!(decode_entry(&row).mnemonic.contains("5A"));
    row.observation.frame_knowledge = TraceFrameKnowledge::Exact;
    assert_eq!(trace_frame_label(&row), "0");
}
