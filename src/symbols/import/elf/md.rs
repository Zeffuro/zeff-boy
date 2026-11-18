use super::*;
use object::{ObjectKind, SectionFlags, SectionKind, SymbolSection, elf};

fn flags<'data>(section: &impl ObjectSection<'data>) -> elf::SectionFlags {
    match section.flags() {
        SectionFlags::Elf { sh_flags, .. } => sh_flags,
        _ => elf::SectionFlags(0),
    }
}

pub(super) fn import(file: &object::File<'_>, ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
    anyhow::ensure!(
        file.architecture() == object::Architecture::M68k
            && !file.is_64()
            && !file.is_little_endian(),
        "Mega Drive ELF requires 32-bit big-endian M68k"
    );
    anyhow::ensure!(
        file.kind() == ObjectKind::Executable,
        "Mega Drive ELF requires linked executable CPU addresses"
    );
    for section in file.sections() {
        let attributes = flags(&section);
        if !attributes.contains(elf::SHF_ALLOC) {
            continue;
        }
        anyhow::ensure!(
            !attributes.contains(elf::SHF_TLS),
            "Mega Drive ELF TLS sections are not qualified"
        );
        crate::symbols::md::validate_cpu_range(section.address(), Some(section.size()))?;
        section.data()?;
    }
    let mut module = SymbolModule::default();
    let mut seen = HashSet::new();
    for symbol in file.symbols() {
        if matches!(
            symbol.kind(),
            ObjectSymbolKind::File | ObjectSymbolKind::Section
        ) || matches!(
            symbol.section(),
            SymbolSection::Undefined | SymbolSection::Common
        ) {
            continue;
        }
        let name = symbol.name()?;
        if name.is_empty() {
            continue;
        }
        anyhow::ensure!(
            name.len() <= 256 && !name.chars().any(char::is_control),
            "invalid Mega Drive ELF symbol name"
        );
        let symbol_kind = match symbol.flags() {
            object::SymbolFlags::Elf { st_info, .. } => match st_info.st_type() {
                elf::STT_FUNC => ObjectSymbolKind::Text,
                elf::STT_OBJECT => ObjectSymbolKind::Data,
                elf::STT_NOTYPE => ObjectSymbolKind::Label,
                kind => anyhow::bail!(
                    "unsupported Mega Drive ELF symbol type {} for {name}",
                    kind.0
                ),
            },
            _ => anyhow::bail!("missing Mega Drive ELF symbol flags for {name}"),
        };
        let address = symbol.address();
        let size = (symbol.size() != 0).then_some(symbol.size());
        let (kind, cpu, value) = match symbol.section() {
            SymbolSection::Absolute => (SymbolKind::Constant, None, Some(address)),
            SymbolSection::Section(index) => {
                let section = file.section_by_index(index)?;
                let attributes = flags(&section);
                if !attributes.contains(elf::SHF_ALLOC) {
                    continue;
                }
                crate::symbols::md::validate_cpu_range(address, size)?;
                let end = section
                    .address()
                    .checked_add(section.size())
                    .ok_or_else(|| anyhow::anyhow!("Mega Drive ELF section range overflows"))?;
                anyhow::ensure!(
                    address >= section.address()
                        && if let Some(size) = size {
                            address.checked_add(size).is_some_and(|last| last <= end)
                        } else {
                            address <= end
                        },
                    "Mega Drive ELF symbol {name} is outside its section"
                );
                let executable = attributes.contains(elf::SHF_EXECINSTR)
                    && section.kind() != SectionKind::UninitializedData;
                let kind = match symbol_kind {
                    ObjectSymbolKind::Text => {
                        anyhow::ensure!(
                            executable
                                && address & 1 == 0
                                && address.checked_add(2).is_some_and(|next| next <= end),
                            "Mega Drive ELF function {name} is not aligned executable code"
                        );
                        SymbolKind::Function
                    }
                    ObjectSymbolKind::Data => SymbolKind::Data,
                    ObjectSymbolKind::Label => {
                        if executable
                            && address & 1 == 0
                            && address.checked_add(2).is_some_and(|next| next <= end)
                        {
                            SymbolKind::Label
                        } else {
                            SymbolKind::Data
                        }
                    }
                    _ => continue,
                };
                (
                    kind,
                    Some(CpuLocation {
                        space: ctx.cpu_space,
                        address,
                    }),
                    None,
                )
            }
            _ => continue,
        };
        if !seen.insert((name.to_owned(), address, kind as u8)) {
            continue;
        }
        module.symbols.push(SymbolRecord {
            id: SymbolId(0),
            name: name.to_owned(),
            location: SymbolLocation {
                cpu,
                storage: None,
                bank: None,
                exec_mode: ExecMode::M68000,
            },
            value,
            size,
            kind,
            scope: if symbol.is_global() {
                SymbolScope::Global
            } else {
                SymbolScope::Local
            },
            provenance: Provenance {
                kind: ProvenanceKind::DebugFormat,
                source: ctx.source_name.clone(),
            },
            confidence: Confidence::Exact,
            comment: None,
        });
    }
    match import_dwarf_lines(file, ctx, &[]) {
        Ok((source_files, source_lines)) => {
            module.source_files = source_files;
            module.source_lines = source_lines;
        }
        Err(error) => module
            .diagnostics
            .push(format!("DWARF line import failed: {error}")),
    }
    anyhow::ensure!(
        !module.symbols.is_empty() || !module.source_lines.is_empty(),
        "no usable M68k ELF debug data found"
    );
    Ok(module)
}

#[cfg(test)]
pub(crate) fn fixture() -> Vec<u8> {
    use object::write::{Object, StandardSection, Symbol, SymbolSection};
    let mut object = Object::new(
        BinaryFormat::Elf,
        object::Architecture::M68k,
        object::Endianness::Big,
    );
    let text = object.section_id(StandardSection::Text);
    object.append_section_data(text, &[0x4e, 0x75, 0x4e, 0x71, 0x70, 7, 0x4e, 0x75], 2);
    let data = object.section_id(StandardSection::Data);
    object.append_section_data(data, &[0xa5, 0x5a], 2);
    let bss = object.section_id(StandardSection::UninitializedData);
    object.append_section_bss(bss, 8, 2);
    for (name, value, size, kind, section, scope) in [
        (
            "Entry",
            0x400,
            4,
            ObjectSymbolKind::Text,
            SymbolSection::Section(text),
            object::SymbolScope::Linkage,
        ),
        (
            "$t",
            0x404,
            0,
            ObjectSymbolKind::Label,
            SymbolSection::Section(text),
            object::SymbolScope::Compilation,
        ),
        (
            "Counter",
            0xff0040,
            1,
            ObjectSymbolKind::Data,
            SymbolSection::Section(data),
            object::SymbolScope::Linkage,
        ),
        (
            "OddData",
            0xff0041,
            1,
            ObjectSymbolKind::Data,
            SymbolSection::Section(data),
            object::SymbolScope::Compilation,
        ),
        (
            "Scratch",
            0xff0081,
            2,
            ObjectSymbolKind::Data,
            SymbolSection::Section(bss),
            object::SymbolScope::Linkage,
        ),
        (
            "End",
            0xff0088,
            0,
            ObjectSymbolKind::Label,
            SymbolSection::Section(bss),
            object::SymbolScope::Linkage,
        ),
        (
            "Mask",
            0xdeadbeef,
            0,
            ObjectSymbolKind::Label,
            SymbolSection::Absolute,
            object::SymbolScope::Linkage,
        ),
        (
            "External",
            0,
            0,
            ObjectSymbolKind::Label,
            SymbolSection::Undefined,
            object::SymbolScope::Linkage,
        ),
    ] {
        object.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value,
            size,
            kind,
            scope,
            weak: false,
            section,
            flags: object::SymbolFlags::None,
        });
    }
    let mut bytes = object.write().unwrap();
    bytes[16..18].copy_from_slice(&elf::ET_EXEC.0.to_be_bytes());
    let sections = object::File::parse(bytes.as_slice()).unwrap();
    let indices =
        [".text", ".data", ".bss"].map(|name| sections.section_by_name(name).unwrap().index().0);
    let start = u32::from_be_bytes(bytes[32..36].try_into().unwrap()) as usize;
    for (index, address) in indices.into_iter().zip([0x400_u32, 0xff0040, 0xff0080]) {
        bytes[start + index * 40 + 12..start + index * 40 + 16]
            .copy_from_slice(&address.to_be_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::import::import_symbols;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolSession};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(3),
            rom_region: RegionId(4),
            cpu_space: AddressSpaceId(2),
            source_name: Some("build/game.elf".into()),
        }
    }

    fn symbol_field(bytes: &[u8], name: &str) -> usize {
        let file = object::File::parse(bytes).unwrap();
        let symbol = file
            .symbols()
            .find(|symbol| symbol.name().unwrap() == name)
            .unwrap();
        let (start, _) = file
            .section_by_name(".symtab")
            .unwrap()
            .file_range()
            .unwrap();
        start as usize + symbol.index().0 * 16
    }

    fn section_field(bytes: &[u8], name: &str) -> usize {
        let index = object::File::parse(bytes)
            .unwrap()
            .section_by_name(name)
            .unwrap()
            .index()
            .0;
        u32::from_be_bytes(bytes[32..36].try_into().unwrap()) as usize + index * 40
    }

    fn foreign(architecture: object::Architecture, endian: object::Endianness) -> Vec<u8> {
        use object::write::{Object, StandardSection, Symbol, SymbolSection};
        let mut object = Object::new(BinaryFormat::Elf, architecture, endian);
        let text = object.section_id(StandardSection::Text);
        object.append_section_data(text, &[0; 8], 2);
        object.add_symbol(Symbol {
            name: b"ArmEntry".to_vec(),
            value: 0x08000401,
            size: 4,
            kind: ObjectSymbolKind::Text,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: object::SymbolFlags::None,
        });
        object.write().unwrap()
    }

    #[test]
    fn md_elf_registry_keeps_linked_cpu_code_data_bss_and_original_provenance() {
        let bytes = fixture();
        for name in ["game.ELF", "game.axf", "renamed.bin"] {
            let module = import_symbols(name, &bytes, &context()).unwrap();
            assert_eq!(module.format, "ELF symbols");
            assert_eq!(module.symbols.len(), 7);
            assert!(module.source_lines.is_empty() && module.source_files.is_empty());
            let symbol = |name| {
                module
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name == name)
                    .unwrap()
            };
            assert_eq!(
                (symbol("Entry").kind, symbol("Entry").size),
                (SymbolKind::Function, Some(4))
            );
            assert_eq!(symbol("Entry").location.cpu.unwrap().address, 0x400);
            assert_eq!(
                (symbol("$t").kind, symbol("$t").scope),
                (SymbolKind::Label, SymbolScope::Local)
            );
            for (name, address) in [
                ("Counter", 0xff0040),
                ("OddData", 0xff0041),
                ("Scratch", 0xff0081),
                ("End", 0xff0088),
            ] {
                assert_eq!(
                    (
                        symbol(name).kind,
                        symbol(name).location.cpu.unwrap().address
                    ),
                    (SymbolKind::Data, address)
                );
            }
            assert_eq!(symbol("Mask").kind, SymbolKind::Constant);
            assert_eq!(symbol("Mask").value, Some(0xdeadbeef));
            assert!(symbol("Mask").location.cpu.is_none());
            for record in &module.symbols {
                assert_eq!(record.location.exec_mode, ExecMode::M68000);
                assert!(record.location.storage.is_none() && record.location.bank.is_none());
                assert!(
                    record
                        .location
                        .cpu
                        .is_none_or(|cpu| cpu.space == AddressSpaceId(2))
                );
                assert_eq!(record.provenance.kind, ProvenanceKind::DebugFormat);
                assert_eq!(record.provenance.source.as_deref(), Some("build/game.elf"));
                assert_eq!(record.confidence, Confidence::Exact);
            }
        }
    }

    #[test]
    fn md_elf_rejects_bad_records_sections_architecture_width_and_byte_order() {
        let good = fixture();
        let mut variants = vec![
            b"\x7fELF".to_vec(),
            good[..40].to_vec(),
            foreign(object::Architecture::Arm, object::Endianness::Little),
            foreign(object::Architecture::M68k, object::Endianness::Little),
        ];
        let mut wide = foreign(object::Architecture::X86_64, object::Endianness::Big);
        wide[18..20].copy_from_slice(&elf::EM_68K.0.to_be_bytes());
        variants.push(wide);
        for value in [0x01000000_u32, 0xffff0400, 0x401, 0x500] {
            let mut bytes = good.clone();
            let entry = symbol_field(&bytes, "Entry");
            bytes[entry + 4..entry + 8].copy_from_slice(&value.to_be_bytes());
            variants.push(bytes);
        }
        for (field, value) in [(8, 16_u32), (14, 0x7777)] {
            let mut bytes = good.clone();
            let entry = symbol_field(&bytes, "Counter");
            if field == 14 {
                bytes[entry + field..entry + field + 2]
                    .copy_from_slice(&(value as u16).to_be_bytes());
            } else {
                bytes[entry + field..entry + field + 4].copy_from_slice(&value.to_be_bytes());
            }
            variants.push(bytes);
        }
        let mut bytes = good.clone();
        let data = symbol_field(&bytes, "Counter");
        bytes[data + 12] = 0x12;
        variants.push(bytes);
        for (name, info) in [("Entry", 0x17), ("Entry", 0x1a), ("Counter", 0x16)] {
            let mut bytes = good.clone();
            let symbol = symbol_field(&bytes, name);
            bytes[symbol + 12] = info;
            variants.push(bytes);
        }
        for (field, value) in [(12, 0x1000000_u32), (16, 0xfffffff0), (20, 0x1000000)] {
            let mut bytes = good.clone();
            let section = section_field(&bytes, ".text");
            bytes[section + field..section + field + 4].copy_from_slice(&value.to_be_bytes());
            variants.push(bytes);
        }
        for kind in [elf::ET_REL, elf::ET_DYN] {
            let mut bytes = good.clone();
            bytes[16..18].copy_from_slice(&kind.0.to_be_bytes());
            variants.push(bytes);
        }
        let mut tls = good.clone();
        let data = section_field(&tls, ".data");
        let flags = (elf::SHF_WRITE | elf::SHF_ALLOC | elf::SHF_TLS).0 as u32;
        tls[data + 8..data + 12].copy_from_slice(&flags.to_be_bytes());
        variants.push(tls);
        let mut end = good.clone();
        let entry = symbol_field(&end, "Entry");
        end[entry + 4..entry + 8].copy_from_slice(&0x408_u32.to_be_bytes());
        end[entry + 8..entry + 12].copy_from_slice(&0_u32.to_be_bytes());
        variants.push(end);
        for bytes in variants {
            assert!(import_symbols("invalid.elf", &bytes, &context()).is_err());
        }
    }

    #[test]
    fn md_elf_does_not_change_arm_thumb_or_foreign_target_routing() {
        let mut gba = context();
        gba.target.system = System::Gba;
        let bytes = foreign(object::Architecture::Arm, object::Endianness::Little);
        let module = import_symbols("arm.elf", &bytes, &gba).unwrap();
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0x08000400);
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x400);
        assert_eq!(module.symbols[0].location.exec_mode, ExecMode::Thumb);
        assert!(import_symbols("md.elf", &fixture(), &gba).is_err());
        let mut gb = context();
        gb.target.system = System::Gb;
        assert_eq!(ElfImporter.probe("md.elf", &fixture(), &gb.target), 0);
        assert!(ElfImporter.import(&fixture(), &gb).is_err());
        assert!(import_symbols("md.elf", &fixture(), &gb).is_err());
    }

    #[test]
    fn md_elf_automatic_and_selected_sidecars_keep_unverified_identity_and_error_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let rom = directory.path().join("game.md");
        let elf = directory.path().join("game.elf");
        std::fs::write(&elf, fixture()).unwrap();
        std::fs::write(directory.path().join("game.sym"), b"00000600 T NmOnly").unwrap();
        let loaded = SymbolSession::load_for_paths(System::Md, &rom, &rom, [1; 32]);
        assert_eq!(loaded.resolve_cpu_name("Entry"), Some(0x400));
        assert_eq!(loaded.resolve_cpu_name("NmOnly"), None);
        assert!(loaded.diagnostics.is_empty());
        assert_eq!(
            loaded.modules[0].identity,
            crate::symbols::identity::IdentityStatus::Unverified
        );
        let selected =
            SymbolSession::load_for_paths_with_sidecar(System::Md, &rom, &rom, [1; 32], &elf);
        assert_eq!(selected.resolve_cpu_name("Scratch"), Some(0xff0081));
        let mut broken = fixture();
        let counter = symbol_field(&broken, "Counter");
        broken[counter + 8..counter + 12].copy_from_slice(&16_u32.to_be_bytes());
        std::fs::write(&elf, broken).unwrap();
        let fallback = SymbolSession::load_for_paths(System::Md, &rom, &rom, [1; 32]);
        assert_eq!(fallback.resolve_cpu_name("Entry"), None);
        assert_eq!(fallback.resolve_cpu_name("NmOnly"), Some(0x600));
        assert!(!fallback.diagnostics.is_empty());
    }
}
