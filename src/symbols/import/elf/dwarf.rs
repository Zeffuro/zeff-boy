use super::*;
use std::borrow::Cow;
use std::collections::HashMap;

#[derive(Clone)]
struct DwarfRow {
    address: u64,
    path: String,
    line: u32,
}

pub(super) fn import_dwarf_lines(
    file: &object::File<'_>,
    ctx: &ImportContext,
    mode_markers: &[(u64, ExecMode)],
) -> anyhow::Result<(Vec<SourceFile>, Vec<SourceLine>)> {
    let sections = gimli::DwarfSections::load(|id| {
        Ok::<_, gimli::Error>(
            file.section_by_name(id.name())
                .map(|section| section.uncompressed_data())
                .transpose()
                .map_err(|_| gimli::Error::Io)?
                .unwrap_or(Cow::Borrowed(&[])),
        )
    })?;
    let endian = if ctx.target.system == System::Md {
        gimli::RunTimeEndian::Big
    } else {
        gimli::RunTimeEndian::Little
    };
    let dwarf = sections.borrow(|section| gimli::EndianSlice::new(section, endian));
    let mut files = Vec::new();
    let mut file_indices = HashMap::new();
    let mut lines = Vec::new();
    let mut units = dwarf.units();

    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        let Some(program) = unit.line_program.clone() else {
            continue;
        };
        if ctx.target.system == System::Md {
            anyhow::ensure!(
                unit.encoding().address_size == 4,
                "Mega Drive DWARF requires 32-bit addresses"
            );
        }
        let mut rows = program.rows();
        let mut pending: Option<DwarfRow> = None;
        while let Some((header, row)) = rows.next_row()? {
            let address = row.address();
            if let Some(previous) = pending.take() {
                append_dwarf_line(
                    previous,
                    address,
                    file,
                    ctx,
                    mode_markers,
                    (&mut files, &mut file_indices, &mut lines),
                )?;
            }
            if row.end_sequence() {
                continue;
            }
            let Some(line) = row.line().and_then(|line| u32::try_from(line.get()).ok()) else {
                continue;
            };
            let Some(path) = dwarf_source_path(&dwarf, &unit, header, row)? else {
                continue;
            };
            pending = Some(DwarfRow {
                address,
                path,
                line,
            });
        }
        if let Some(previous) = pending {
            anyhow::ensure!(
                ctx.target.system != System::Md,
                "unterminated Mega Drive DWARF line sequence"
            );
            append_dwarf_line(
                previous.clone(),
                previous.address.saturating_add(1),
                file,
                ctx,
                mode_markers,
                (&mut files, &mut file_indices, &mut lines),
            )?;
        }
    }

    lines.sort_unstable_by_key(|line| {
        (
            line.location.cpu.map_or(0, |cpu| cpu.address),
            line.source_file,
            line.line,
        )
    });
    lines.dedup_by(|left, right| {
        left.location == right.location
            && left.size == right.size
            && left.source_file == right.source_file
            && left.line == right.line
    });
    Ok((files, lines))
}

fn dwarf_source_path<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    header: &gimli::LineProgramHeader<R>,
    row: &gimli::LineRow,
) -> gimli::Result<Option<String>> {
    let Some(file) = row.file(header) else {
        return Ok(None);
    };
    let name = dwarf
        .attr_string(unit, file.path_name())?
        .to_string_lossy()?
        .into_owned();
    if name.is_empty() {
        return Ok(None);
    }
    let directory = file
        .directory(header)
        .map(|directory| dwarf.attr_string(unit, directory))
        .transpose()?
        .map(|directory| directory.to_string_lossy().map(Cow::into_owned))
        .transpose()?;
    Ok(Some(join_source_path(directory.as_deref(), &name)))
}

fn join_source_path(directory: Option<&str>, name: &str) -> String {
    if name.starts_with(['/', '\\']) || name.as_bytes().get(1) == Some(&b':') {
        return name.to_owned();
    }
    let Some(directory) = directory.filter(|directory| !directory.is_empty()) else {
        return name.to_owned();
    };
    format!(
        "{}/{}",
        directory.trim_end_matches(['/', '\\']),
        name.trim_start_matches(['/', '\\'])
    )
}

fn append_dwarf_line(
    row: DwarfRow,
    end: u64,
    file: &object::File<'_>,
    ctx: &ImportContext,
    mode_markers: &[(u64, ExecMode)],
    output: (
        &mut Vec<SourceFile>,
        &mut HashMap<String, usize>,
        &mut Vec<SourceLine>,
    ),
) -> anyhow::Result<()> {
    let (files, file_indices, lines) = output;
    let location = if ctx.target.system == System::Md {
        if end == row.address {
            return Ok(());
        }
        md_location(file, row.address, end, ctx)?
    } else {
        if row.address > u64::from(u32::MAX) || end <= row.address {
            return Ok(());
        }
        let exec_mode = exec_mode(SymbolKind::Function, row.address, row.address, mode_markers);
        let address = if exec_mode == ExecMode::Thumb {
            row.address & !1
        } else {
            row.address
        };
        SymbolLocation {
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
        }
    };
    let source_file = *file_indices.entry(row.path.clone()).or_insert_with(|| {
        let index = files.len();
        files.push(SourceFile {
            path: row.path,
            crc32: None,
        });
        index
    });
    lines.push(SourceLine {
        location,
        size: end.saturating_sub(row.address),
        source_file,
        line: row.line,
    });
    Ok(())
}

fn md_location(
    file: &object::File<'_>,
    start: u64,
    end: u64,
    ctx: &ImportContext,
) -> anyhow::Result<SymbolLocation> {
    let size = end
        .checked_sub(start)
        .filter(|&size| size >= 2)
        .ok_or_else(|| anyhow::anyhow!("invalid Mega Drive DWARF instruction span"))?;
    crate::symbols::md::validate_cpu_range(start, Some(size))?;
    anyhow::ensure!(
        start & 1 == 0 && end & 1 == 0,
        "unaligned Mega Drive DWARF instruction span"
    );
    let mut populated = false;
    for section in file.sections() {
        let flags = match section.flags() {
            object::SectionFlags::Elf { sh_flags } => sh_flags,
            _ => 0,
        };
        let required = u64::from(object::elf::SHF_ALLOC | object::elf::SHF_EXECINSTR);
        if flags & required != required
            || flags & u64::from(object::elf::SHF_COMPRESSED) != 0
            || section.kind() != object::SectionKind::Text
        {
            continue;
        }
        let section_end = section
            .address()
            .checked_add(section.size())
            .ok_or_else(|| anyhow::anyhow!("Mega Drive DWARF section range overflows"))?;
        if start >= section.address() && end <= section_end {
            let data = section.data()?;
            if end - section.address() <= data.len() as u64 {
                populated = true;
                break;
            }
        }
    }
    anyhow::ensure!(
        populated,
        "Mega Drive DWARF span is not within populated executable code"
    );
    Ok(SymbolLocation {
        cpu: Some(CpuLocation {
            space: ctx.cpu_space,
            address: start,
        }),
        storage: None,
        bank: None,
        exec_mode: ExecMode::M68000,
    })
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
