use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Sections,
};
use gimli::{Encoding, Format, LineEncoding, RunTimeEndian};
use object::write::{Object, StandardSection, Symbol, SymbolSection};
use object::{Object as _, ObjectSection as _};

pub(crate) type Sequence<'a> = (u64, &'a [(u64, u64)], u64);

pub(crate) fn valid() -> Vec<u8> {
    build(
        &[(0x400, &[(0, 10), (4, 11)], 8)],
        RunTimeEndian::Big,
        "src/main.asm",
        true,
    )
}

pub(crate) fn build(
    sequences: &[Sequence<'_>],
    endian: RunTimeEndian,
    path: &str,
    symbols: bool,
) -> Vec<u8> {
    let encoding = Encoding {
        format: Format::Dwarf32,
        version: 4,
        address_size: 4,
    };
    let mut dwarf = DwarfUnit::new(encoding);
    let directory = dwarf.strings.add(b".".to_vec());
    let name = dwarf.strings.add(path.as_bytes().to_vec());
    let root = dwarf.unit.root();
    dwarf
        .unit
        .get_mut(root)
        .set(gimli::DW_AT_comp_dir, AttributeValue::StringRef(directory));
    dwarf
        .unit
        .get_mut(root)
        .set(gimli::DW_AT_name, AttributeValue::StringRef(name));
    let mut program = LineProgram::new(
        encoding,
        LineEncoding::default(),
        LineString::new(b".".to_vec(), encoding, &mut dwarf.line_strings),
        None,
        LineString::new(path.as_bytes().to_vec(), encoding, &mut dwarf.line_strings),
        None,
    );
    let file = program.add_file(
        LineString::new(path.as_bytes().to_vec(), encoding, &mut dwarf.line_strings),
        program.default_directory(),
        None,
    );
    for &(start, rows, end) in sequences {
        program.begin_sequence(Some(Address::Constant(start)));
        for &(offset, line) in rows {
            program.row().file = file;
            program.row().line = line;
            program.row().address_offset = offset;
            program.generate_row();
        }
        program.end_sequence(end);
    }
    dwarf.unit.line_program = program;
    let mut sections = Sections::new(EndianVec::new(endian));
    dwarf.write(&mut sections).unwrap();
    let mut object = Object::new(
        object::BinaryFormat::Elf,
        object::Architecture::M68k,
        object::Endianness::Big,
    );
    let text = object.section_id(StandardSection::Text);
    object.append_section_data(text, &[0x4e, 0x71, 0x70, 7, 0x4e, 0x75, 0x4e, 0x71], 2);
    let data = object.section_id(StandardSection::Data);
    object.append_section_data(data, &[0xa5, 0x5a], 2);
    let bss = object.section_id(StandardSection::UninitializedData);
    object.append_section_bss(bss, 8, 2);
    let top = object.add_section(Vec::new(), b".top".to_vec(), object::SectionKind::Text);
    object.append_section_data(top, &[0x4e, 0x71], 2);
    let bare = object.add_section(Vec::new(), b".bare".to_vec(), object::SectionKind::Text);
    object.append_section_data(bare, &[0x4e, 0x71], 2);
    object.section_mut(bare).flags = object::SectionFlags::Elf {
        sh_type: object::elf::SHT_PROGBITS,
        sh_flags: object::elf::SHF_EXECINSTR,
    };
    if symbols {
        for (name, value, size, kind, section) in [
            ("Entry", 0x400, 8, object::SymbolKind::Text, text),
            ("Counter", 0xff0040, 1, object::SymbolKind::Data, data),
        ] {
            object.add_symbol(Symbol {
                name: name.as_bytes().to_vec(),
                value,
                size,
                kind,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Section(section),
                flags: object::SymbolFlags::None,
            });
        }
    }
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
    let mut bytes = object.write().unwrap();
    bytes[16..18].copy_from_slice(&object::elf::ET_EXEC.0.to_be_bytes());
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let indices = [".text", ".data", ".bss", ".top", ".bare"]
        .map(|name| file.section_by_name(name).unwrap().index().0);
    let table = u32::from_be_bytes(bytes[32..36].try_into().unwrap()) as usize;
    for (index, address) in indices
        .into_iter()
        .zip([0x400_u32, 0xff0040, 0xff0080, 0xfffffe, 0x600])
    {
        bytes[table + index * 40 + 12..table + index * 40 + 16]
            .copy_from_slice(&address.to_be_bytes());
    }
    bytes
}

pub(super) fn section_field(bytes: &[u8], name: &str) -> usize {
    let index = object::File::parse(bytes)
        .unwrap()
        .section_by_name(name)
        .unwrap()
        .index()
        .0;
    u32::from_be_bytes(bytes[32..36].try_into().unwrap()) as usize + index * 40
}

pub(super) fn truncate_line_section(bytes: &mut [u8]) {
    let field = section_field(bytes, ".debug_line");
    let size = u32::from_be_bytes(bytes[field + 20..field + 24].try_into().unwrap());
    bytes[field + 20..field + 24].copy_from_slice(&(size - 1).to_be_bytes());
}
