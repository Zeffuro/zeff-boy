use super::*;
use crate::debug::{DisassembledLine, DisassemblyCursor, DisassemblyView};
use crate::symbols::{SourceFile, SourceLine};

fn view(offset: u64) -> DisassemblyView {
    let mut view = DisassemblyView::unavailable_rom(offset);
    view.lines.push(DisassembledLine {
        address: offset.try_into().unwrap(),
        decode_status: Some(zeff_m68k::disassembly::DecodeStatus::Complete),
        storage_offset: Some(offset),
        bank: None,
        symbol: None,
        control_target: None,
        control_target_storage: None,
        control_target_bank: None,
        control_target_symbol: None,
        source: None,
        bytes: [0x4e, 0x71].into_iter().collect(),
        mnemonic: "NOP".try_into().unwrap(),
    });
    view
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_source_disassembly_uses_only_explicit_original_storage_symbols_and_source() {
    let mut symbols = session();
    let cpu_location = symbols.store.lookup_name("Entry").next().unwrap().location;
    symbols.extend_source_metadata(
        vec![SourceFile {
            path: "cpu.asm".into(),
            crc32: None,
        }],
        vec![SourceLine {
            location: cpu_location,
            size: 8,
            source_file: 0,
            line: 10,
        }],
        None,
    );
    let mut source = view(0x123400);
    symbols.segments.push(crate::symbols::DebugSegment {
        id: crate::symbols::SegmentId(1),
        name: "loaded".into(),
        storage: crate::symbols::StorageRange {
            start: StorageLocation {
                image: ImageId(0),
                region: RegionId(0),
                offset: 0x8000,
            },
            size: 8,
        },
        linked_cpu: None,
        exec_mode: ExecMode::M68000,
    });
    symbols.load_instances.push(crate::symbols::LoadInstance {
        id: crate::symbols::LoadInstanceId(1),
        segment: crate::symbols::SegmentId(1),
        runtime_base: CpuLocation {
            space: AddressSpaceId(0),
            address: 0x123400,
        },
        generation: 1,
        created_cycle: 1,
        active: true,
    });
    let mut no_storage = source.clone();
    no_storage.lines[0].storage_offset = None;
    symbols.annotate_disassembly(&mut no_storage);
    assert!(no_storage.lines[0].storage_offset.is_none());
    assert!(no_storage.lines[0].symbol.is_none() && no_storage.lines[0].source.is_none());
    symbols.annotate_disassembly(&mut source);
    assert!(source.lines[0].symbol.is_none() && source.lines[0].source.is_none());
    assert!(source.location_symbol.is_none());
    assert!(symbols.source_reference_for_disassembly(&source).is_none());
    let mut cpu = source.clone();
    cpu.cursor = DisassemblyCursor::Cpu(0x123400);
    cpu.lines[0].storage_offset = None;
    symbols.annotate_disassembly(&mut cpu);
    assert_eq!(cpu.lines[0].symbol.as_deref(), Some("Entry"));
    assert_eq!(cpu.lines[0].storage_offset, Some(0x8000));
    assert_eq!(cpu.lines[0].source.as_deref(), Some("cpu.asm:10"));
    let mut record = symbols.store.lookup_name("Entry").next().unwrap().clone();
    record.name = "OriginalCode".into();
    record.location.cpu = None;
    record.location.storage = Some(StorageLocation {
        image: ImageId(0),
        region: RegionId(0),
        offset: 0x123400,
    });
    symbols.store.insert(record.clone());
    symbols.extend_source_metadata(
        vec![SourceFile {
            path: "original.asm".into(),
            crc32: None,
        }],
        vec![SourceLine {
            location: record.location,
            size: 8,
            source_file: 0,
            line: 42,
        }],
        None,
    );
    symbols.annotate_disassembly(&mut source);
    assert_eq!(source.lines[0].symbol.as_deref(), Some("OriginalCode"));
    assert_eq!(source.location_symbol.as_deref(), Some("OriginalCode"));
    assert_eq!(source.lines[0].source.as_deref(), Some("original.asm:42"));
    assert_eq!(
        symbols
            .source_reference_for_disassembly(&source)
            .unwrap()
            .line,
        42
    );
    assert!(source.lines[0].control_target.is_none() && source.lines[0].bank.is_none());
    assert!(
        symbols
            .source_reference_for_disassembly(&DisassemblyView::unavailable_rom(u64::MAX))
            .is_none()
    );
}
