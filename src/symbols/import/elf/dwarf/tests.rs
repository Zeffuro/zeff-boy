use super::*;
use crate::symbols::import::import_symbols;
use crate::symbols::{AddressSpaceId, ImageId, RegionId};
use fixture::{build, valid};
use gimli::RunTimeEndian;

fn context() -> ImportContext {
    ImportContext {
        target: TargetInfo { system: System::Md },
        image: ImageId(3),
        rom_region: RegionId(4),
        cpu_space: AddressSpaceId(2),
        source_name: Some("build/game.elf".into()),
    }
}

#[test]
fn md_big_endian_dwarf_rows_keep_exact_cpu_spans_paths_and_end_sequences() {
    let bytes = build(
        &[(0x400, &[(0, 10), (4, 11)], 8), (0xfffffe, &[(0, 20)], 2)],
        RunTimeEndian::Big,
        "src/main.asm",
        true,
    );
    let module = import_symbols("game.elf", &bytes, &context()).unwrap();
    assert!(module.diagnostics.is_empty());
    assert_eq!(module.symbols.len(), 2);
    assert_eq!(module.source_files.len(), 1);
    assert_eq!(module.source_files[0].path, "./src/main.asm");
    assert_eq!(module.source_files[0].crc32, None);
    assert_eq!(
        module
            .source_lines
            .iter()
            .map(|line| (line.location.cpu.unwrap().address, line.size, line.line))
            .collect::<Vec<_>>(),
        [(0x400, 4, 10), (0x404, 4, 11), (0xfffffe, 2, 20)]
    );
    for line in &module.source_lines {
        assert_eq!(line.location.cpu.unwrap().space, AddressSpaceId(2));
        assert_eq!(line.location.exec_mode, ExecMode::M68000);
        assert!(line.location.storage.is_none() && line.location.bank.is_none());
        assert_eq!(line.source_file, 0);
    }
    let absolute = build(
        &[(0x400, &[(0, 5)], 2)],
        RunTimeEndian::Big,
        "/project/main.asm",
        true,
    );
    let module = import_symbols("game.elf", &absolute, &context()).unwrap();
    assert_eq!(module.source_files[0].path, "/project/main.asm");
}

#[test]
fn md_dwarf_rejects_noninstruction_spans_atomically_but_keeps_valid_symbols() {
    for (start, end) in [
        (0x401, 0x403),
        (0x400, 0x401),
        (0x406, 0x40a),
        (0xff0040, 0xff0042),
        (0xff0080, 0xff0082),
        (0x600, 0x602),
        (0x700, 0x702),
        (0x1000000, 0x1000002),
        (0xfffffe, 0x1000002),
    ] {
        let bytes = build(
            &[(0x400, &[(0, 10)], 2), (start, &[(0, 20)], end - start)],
            RunTimeEndian::Big,
            "src/main.asm",
            true,
        );
        let module = import_symbols("game.elf", &bytes, &context()).unwrap();
        assert_eq!(module.symbols.len(), 2, "{start:x}..{end:x}");
        assert!(
            module.source_files.is_empty() && module.source_lines.is_empty(),
            "{start:x}..{end:x}"
        );
        assert_eq!(module.diagnostics.len(), 1);
        assert!(module.diagnostics[0].starts_with("DWARF line import failed:"));
        assert_eq!(
            module
                .symbols
                .iter()
                .find(|symbol| symbol.name == "Entry")
                .unwrap()
                .location
                .cpu
                .unwrap()
                .address,
            0x400
        );
    }
    let mut bytes = build(
        &[(0xff0080, &[(0, 10)], 2)],
        RunTimeEndian::Big,
        "src/main.asm",
        true,
    );
    let bss = fixture::section_field(&bytes, ".bss");
    bytes[bss + 8..bss + 12]
        .copy_from_slice(&(object::elf::SHF_ALLOC | object::elf::SHF_EXECINSTR).to_be_bytes());
    let module = import_symbols("bss.elf", &bytes, &context()).unwrap();
    assert!(module.source_lines.is_empty());
    assert_eq!(module.diagnostics.len(), 1);
    let file = object::File::parse(bytes.as_slice()).unwrap();
    assert!(md_location(&file, 0xfffffe, u64::MAX, &context()).is_err());
    assert!(md_location(&file, 0x404, 0x400, &context()).is_err());
}

#[test]
fn malformed_md_dwarf_endianness_truncation_and_missing_end_keep_symbol_import() {
    let mut truncated = valid();
    fixture::truncate_line_section(&mut truncated);
    let wrong_endian = build(
        &[(0x400, &[(0, 10)], 4)],
        RunTimeEndian::Little,
        "src/main.asm",
        true,
    );
    let mut missing_end = valid();
    let file = object::File::parse(missing_end.as_slice()).unwrap();
    let (offset, size) = file
        .section_by_name(".debug_line")
        .unwrap()
        .file_range()
        .unwrap();
    let last = offset as usize + size as usize - 1;
    assert_eq!(missing_end[last], gimli::DW_LNE_end_sequence.0);
    missing_end[last] = 0xff;
    for bytes in [truncated, wrong_endian, missing_end] {
        let module = import_symbols("game.elf", &bytes, &context()).unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert!(module.source_lines.is_empty() && module.source_files.is_empty());
        assert_eq!(module.diagnostics.len(), 1);
    }
}

#[test]
fn md_line_only_debug_data_is_real_and_empty_debug_data_is_rejected() {
    let bytes = build(
        &[(0x400, &[(0, 10), (0, 11), (2, 12)], 4)],
        RunTimeEndian::Big,
        "src/main.asm",
        false,
    );
    let module = import_symbols("lines.elf", &bytes, &context()).unwrap();
    assert!(module.symbols.is_empty() && module.diagnostics.is_empty());
    assert_eq!(
        module
            .source_lines
            .iter()
            .map(|row| (row.line, row.size))
            .collect::<Vec<_>>(),
        [(11, 2), (12, 2)]
    );
    let bytes = build(&[], RunTimeEndian::Big, "src/main.asm", false);
    assert!(import_symbols("empty.elf", &bytes, &context()).is_err());
    let mut broken = build(
        &[(0x400, &[(0, 10)], 2)],
        RunTimeEndian::Big,
        "src/main.asm",
        false,
    );
    fixture::truncate_line_section(&mut broken);
    assert!(import_symbols("broken.elf", &broken, &context()).is_err());
}

#[test]
fn md_malformed_line_sidecar_keeps_symbols_and_reports_zero_source_rows() {
    let directory = tempfile::tempdir().unwrap();
    let rom = directory.path().join("game.md");
    let elf = directory.path().join("game.elf");
    let mut bytes = valid();
    fixture::truncate_line_section(&mut bytes);
    std::fs::write(&elf, bytes).unwrap();
    std::fs::write(directory.path().join("game.sym"), "00000600 T NmOnly").unwrap();
    let session = crate::symbols::SymbolSession::load_for_paths(System::Md, &rom, &rom, [9; 32]);
    assert_eq!(session.resolve_cpu_name("Entry"), Some(0x400));
    assert_eq!(session.resolve_cpu_name("Counter"), Some(0xff0040));
    assert_eq!(session.resolve_cpu_name("NmOnly"), None);
    assert_eq!(session.modules[0].source_line_count, 0);
    assert_eq!(
        session.modules[0].identity,
        crate::symbols::identity::IdentityStatus::Unverified
    );
    assert_eq!(session.modules[0].diagnostics.len(), 1);
    assert!(session.modules[0].diagnostics[0].starts_with("DWARF line import failed:"));
}
