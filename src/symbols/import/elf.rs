use std::collections::HashSet;

use object::{BinaryFormat, Object, ObjectSection, ObjectSymbol, SymbolKind as ObjectSymbolKind};
use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SourceFile, SourceLine,
    StorageLocation, SymbolId, SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

mod dwarf;
mod md;
use dwarf::import_dwarf_lines;

#[cfg(test)]
pub(crate) use dwarf::fixture::valid as md_dwarf_fixture;

#[cfg(test)]
pub(crate) use md::fixture as md_elf_fixture;

pub(crate) struct ElfImporter;

impl SymbolImporter for ElfImporter {
    fn name(&self) -> &'static str {
        "ELF symbols"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if !matches!(target.system, System::Gba | System::Md) || !data.starts_with(b"\x7FELF") {
            return 0;
        }
        let extension = file_name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("elf" | "axf")) {
            100
        } else {
            90
        }
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS.union(ImportCapabilities::SOURCE_LINES)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            matches!(ctx.target.system, System::Gba | System::Md),
            "ELF symbols require a GBA or Mega Drive target"
        );
        let file = object::File::parse(data)?;
        anyhow::ensure!(file.format() == BinaryFormat::Elf, "symbol file is not ELF");
        if ctx.target.system == System::Md {
            return md::import(&file, ctx);
        }
        anyhow::ensure!(
            file.architecture() == object::Architecture::Arm,
            "ELF architecture is not 32-bit ARM"
        );

        let mode_markers = file
            .symbols()
            .filter_map(|symbol| {
                let name = symbol.name().ok()?;
                marker_mode(name).map(|mode| (symbol.address() & !1, mode))
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        let mut module = SymbolModule::default();

        for symbol in file.symbols() {
            if !symbol.is_definition() || symbol.address() > u64::from(u32::MAX) {
                continue;
            }
            let Ok(name) = symbol.name() else {
                continue;
            };
            if name.is_empty() || marker_mode(name).is_some() {
                continue;
            }
            let Some(kind) = symbol_kind(symbol.kind()) else {
                continue;
            };
            let raw_address = symbol.address();
            let address = if matches!(kind, SymbolKind::Function | SymbolKind::Label) {
                raw_address & !1
            } else {
                raw_address
            };
            if !seen.insert((name.to_owned(), address)) {
                continue;
            }
            let exec_mode = exec_mode(kind, raw_address, address, &mode_markers);
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: name.to_owned(),
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address,
                    }),
                    storage: gba_rom_offset(address).map(|offset| StorageLocation {
                        image: ctx.image,
                        region: ctx.rom_region,
                        offset,
                    }),
                    bank: None,
                    exec_mode,
                },
                value: None,
                size: (symbol.size() != 0).then_some(symbol.size()),
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

        match import_dwarf_lines(&file, ctx, &mode_markers) {
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
            "no usable ELF debug data found"
        );
        Ok(module)
    }
}

fn symbol_kind(kind: ObjectSymbolKind) -> Option<SymbolKind> {
    match kind {
        ObjectSymbolKind::Text => Some(SymbolKind::Function),
        ObjectSymbolKind::Data | ObjectSymbolKind::Tls => Some(SymbolKind::Data),
        ObjectSymbolKind::Label | ObjectSymbolKind::Unknown => Some(SymbolKind::Label),
        _ => None,
    }
}

fn marker_mode(name: &str) -> Option<ExecMode> {
    match name.strip_prefix('$')?.chars().next()? {
        'a' => Some(ExecMode::Arm),
        't' => Some(ExecMode::Thumb),
        'd' => Some(ExecMode::Unknown),
        _ => None,
    }
}

fn exec_mode(
    kind: SymbolKind,
    raw_address: u64,
    address: u64,
    markers: &[(u64, ExecMode)],
) -> ExecMode {
    if kind == SymbolKind::Data {
        return ExecMode::Unknown;
    }
    if raw_address & 1 != 0 {
        return ExecMode::Thumb;
    }
    markers
        .iter()
        .filter(|(marker_address, _)| *marker_address <= address)
        .max_by_key(|(marker_address, _)| *marker_address)
        .map_or(ExecMode::Arm, |(_, mode)| *mode)
}

fn gba_rom_offset(address: u64) -> Option<u64> {
    (0x0800_0000..=0x0DFF_FFFF)
        .contains(&address)
        .then(|| (address - 0x0800_0000) & 0x01FF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo {
                system: System::Gba,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.elf".into()),
        }
    }

    #[test]
    fn rejects_non_elf_data() {
        assert!(ElfImporter.import(b"not elf", &context()).is_err());
    }

    #[test]
    fn normalizes_modes_and_rom_mirrors() {
        assert_eq!(
            exec_mode(SymbolKind::Function, 0x0800_0101, 0x0800_0100, &[]),
            ExecMode::Thumb
        );
        assert_eq!(
            exec_mode(SymbolKind::Function, 0x0800_0200, 0x0800_0200, &[]),
            ExecMode::Arm
        );
        assert_eq!(gba_rom_offset(0x0A00_0100), Some(0x100));
    }

    #[test]
    fn imports_elf_symbol_table() {
        let data = elf_fixture();
        assert_eq!(ElfImporter.probe("game.elf", &data, &context().target), 100);
        let module = ElfImporter.import(&data, &context()).unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].name, "Init");
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0x0800_0100);
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x100);
        assert_eq!(module.symbols[0].location.exec_mode, ExecMode::Thumb);
        assert_eq!(module.symbols[0].kind, SymbolKind::Function);
        assert_eq!(module.symbols[1].name, "gData");
        assert_eq!(module.symbols[1].kind, SymbolKind::Data);
    }

    #[test]
    fn imports_dwarf_line_ranges() {
        let data = elf_dwarf_fixture();
        let module = ElfImporter.import(&data, &context()).unwrap();
        assert_eq!(module.source_files[0].path, "/project/src/main.c");
        assert_eq!(module.source_lines.len(), 2);
        assert_eq!(module.source_lines[0].line, 10);
        assert_eq!(module.source_lines[0].size, 4);
        assert_eq!(
            module.source_lines[0].location.cpu.unwrap().address,
            0x0800_0100
        );
        assert_eq!(
            module.source_lines[0].location.storage.unwrap().offset,
            0x100
        );
        assert_eq!(module.source_lines[1].line, 11);
        assert_eq!(module.source_lines[1].size, 4);
    }

    fn elf_dwarf_fixture() -> Vec<u8> {
        use gimli::write::{
            Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Sections,
        };
        use gimli::{Encoding, Format, LineEncoding, LittleEndian};
        use object::write::{Object, Symbol, SymbolSection};

        let encoding = Encoding {
            format: Format::Dwarf32,
            version: 4,
            address_size: 4,
        };
        let mut dwarf = DwarfUnit::new(encoding);
        let comp_dir = dwarf.strings.add(b"/project".to_vec());
        let comp_name = dwarf.strings.add(b"src/main.c".to_vec());
        let root = dwarf.unit.root();
        let entry = dwarf.unit.get_mut(root);
        entry.set(gimli::DW_AT_comp_dir, AttributeValue::StringRef(comp_dir));
        entry.set(gimli::DW_AT_name, AttributeValue::StringRef(comp_name));

        let mut program = LineProgram::new(
            encoding,
            LineEncoding::default(),
            LineString::new(b"/project".to_vec(), encoding, &mut dwarf.line_strings),
            Some(LineString::new(
                b"src".to_vec(),
                encoding,
                &mut dwarf.line_strings,
            )),
            LineString::new(b"main.c".to_vec(), encoding, &mut dwarf.line_strings),
            None,
        );
        let directory = program.default_directory();
        let file = program.add_file(
            LineString::new(b"src/main.c".to_vec(), encoding, &mut dwarf.line_strings),
            directory,
            None,
        );
        program.begin_sequence(Some(Address::Constant(0x0800_0100)));
        program.row().file = file;
        program.row().line = 10;
        program.generate_row();
        program.row().address_offset = 4;
        program.row().line = 11;
        program.generate_row();
        program.end_sequence(8);
        dwarf.unit.line_program = program;

        let mut sections = Sections::new(EndianVec::new(LittleEndian));
        dwarf.write(&mut sections).unwrap();

        let mut object = Object::new(
            object::BinaryFormat::Elf,
            object::Architecture::Arm,
            object::Endianness::Little,
        );
        let text = object.section_id(object::write::StandardSection::Text);
        object.append_section_data(text, &[0; 8], 4);
        object.add_symbol(Symbol {
            name: b"Init".to_vec(),
            value: 1,
            size: 8,
            kind: object::SymbolKind::Text,
            scope: object::SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: object::SymbolFlags::None,
        });
        sections
            .for_each(|id, data| {
                if !data.slice().is_empty() {
                    let section = object.add_section(
                        Vec::new(),
                        id.name().as_bytes().to_vec(),
                        object::SectionKind::Debug,
                    );
                    object.append_section_data(section, data.slice(), 1);
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        object.write().unwrap()
    }

    fn elf_fixture() -> Vec<u8> {
        let mut data = vec![0; 0x248];
        data[..7].copy_from_slice(b"\x7FELF\x01\x01\x01");
        put16(&mut data, 16, 2);
        put16(&mut data, 18, 40);
        put32(&mut data, 20, 1);
        put32(&mut data, 24, 0x0800_0101);
        put32(&mut data, 32, 0x180);
        put32(&mut data, 36, 0x0500_0200);
        put16(&mut data, 40, 52);
        put16(&mut data, 46, 40);
        put16(&mut data, 48, 5);
        put16(&mut data, 50, 4);

        data[0x80..0xA0].fill(0xEA);
        data[0xA0..0xAF].copy_from_slice(b"\0$t\0Init\0gData\0");
        put_symbol(&mut data, 0xC0, [1, 0x0800_0100, 0, 0x00, 1]);
        put_symbol(&mut data, 0xD0, [4, 0x0800_0101, 0x20, 0x12, 1]);
        put_symbol(&mut data, 0xE0, [9, 0x0200_0000, 4, 0x11, 1]);
        data[0xF0..0x111].copy_from_slice(b"\0.text\0.strtab\0.symtab\0.shstrtab\0");

        put_section(
            &mut data,
            0x1A8,
            [1, 1, 6, 0x0800_0100, 0x80, 0x20, 0, 0, 4, 0],
        );
        put_section(&mut data, 0x1D0, [7, 3, 0, 0, 0xA0, 0x0F, 0, 0, 1, 0]);
        put_section(&mut data, 0x1F8, [15, 2, 0, 0, 0xB0, 0x40, 2, 2, 4, 16]);
        put_section(&mut data, 0x220, [23, 3, 0, 0, 0xF0, 0x21, 0, 0, 1, 0]);
        data
    }

    fn put_symbol(data: &mut [u8], offset: usize, values: [u32; 5]) {
        let [name, value, size, info, section] = values;
        put32(data, offset, name);
        put32(data, offset + 4, value);
        put32(data, offset + 8, size);
        data[offset + 12] = info as u8;
        put16(data, offset + 14, section as u16);
    }

    fn put_section(data: &mut [u8], offset: usize, values: [u32; 10]) {
        for (index, value) in values.into_iter().enumerate() {
            put32(data, offset + index * 4, value);
        }
    }

    fn put16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
