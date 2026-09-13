use std::fmt::Write;

use super::{
    EntryPoint, FileSpan, MalformedInput, NsfRegion, Rate, RipDetails, RipFormat, RipWarning,
};

type Header = (
    FileSpan,
    Option<FileSpan>,
    u16,
    EntryPoint,
    EntryPoint,
    RipDetails,
);

pub(crate) fn text(
    bytes: &[u8],
    at: usize,
    field: &'static str,
    format: RipFormat,
    warnings: &mut Vec<RipWarning>,
) -> String {
    let raw = &bytes[at..at + 32];
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(32);
    if end == 32 && format == RipFormat::Nsf {
        warnings.push(RipWarning::UnterminatedText { field });
    }
    if raw[end..].iter().any(|&byte| byte != 0) {
        warnings.push(RipWarning::NonzeroTextPadding { field });
    }
    if raw[..end].iter().any(|&byte| byte >= 128) {
        warnings.push(RipWarning::NonAsciiText { field });
    }
    if raw[..end].iter().any(|&byte| byte < 32 || byte == 127) {
        warnings.push(RipWarning::ControlText { field });
    }
    let mut display = String::new();
    for &byte in &raw[..end] {
        if byte == b'\\' {
            display.push_str("\\\\");
        } else if (32..127).contains(&byte) {
            display.push(char::from(byte));
        } else {
            write!(display, "\\x{byte:02X}").expect("String writes cannot fail");
        }
    }
    display
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub(crate) fn header(
    bytes: &[u8],
    format: RipFormat,
    warnings: &mut Vec<RipWarning>,
) -> Result<Header, MalformedInput> {
    match format {
        RipFormat::Gbs => gbs(bytes, warnings),
        RipFormat::Nsf => nsf(bytes, warnings),
    }
}

fn entry(
    cpu_address: u16,
    source_offset: Option<u32>,
    name: &'static str,
    warnings: &mut Vec<RipWarning>,
) -> EntryPoint {
    if source_offset.is_none() {
        warnings.push(RipWarning::UnbackedEntry {
            entry: name,
            cpu_address,
        });
    }
    EntryPoint {
        cpu_address,
        initial_source_offset: source_offset,
    }
}

fn gbs(bytes: &[u8], warnings: &mut Vec<RipWarning>) -> Result<Header, MalformedInput> {
    let load = word(bytes, 6);
    let init = word(bytes, 8);
    let play = word(bytes, 10);
    if [load, init, play]
        .iter()
        .any(|address| !(0x400..=0x7fff).contains(address))
    {
        return Err(MalformedInput::InvalidAddress);
    }
    let program = FileSpan {
        offset: 0x70,
        byte_len: (bytes.len() - 0x70) as u32,
    };
    let source_offset = |address: u16| {
        u32::from(address)
            .checked_sub(u32::from(load))
            .filter(|&offset| offset < program.byte_len)
            .map(|offset| program.offset + offset)
    };
    let init = entry(init, source_offset(init), "init", warnings);
    let play = entry(play, source_offset(play), "play", warnings);
    let modulo = bytes[0x0e];
    let control = bytes[0x0f];
    if control & 0x38 != 0 {
        warnings.push(RipWarning::ReservedBits {
            field: "timer_control",
            value: control & 0x38,
        });
    }
    if control & 0x40 != 0 {
        warnings.push(RipWarning::CustomInterruptVectors);
        if program.byte_len < 0x50 {
            warnings.push(RipWarning::UnbackedInterruptVectors);
        }
    }
    let double_speed = control & 0x80 != 0;
    let initial_play_rate_hz = if control & 0x78 != 0 {
        None
    } else if control & 4 == 0 {
        Some(Rate {
            numerator: 4_194_304,
            denominator: 70_224,
        })
    } else {
        Some(Rate {
            numerator: [4096, 262_144, 65_536, 16_384][usize::from(control & 3)]
                * if double_speed { 2 } else { 1 },
            denominator: 256 - u32::from(modulo),
        })
    };
    let page_count = (u32::from(load) + program.byte_len).div_ceil(0x4000);
    if page_count > 256 {
        warnings.push(RipWarning::PageIndexExceedsByte { pages: page_count });
    }
    Ok((
        program,
        None,
        load,
        init,
        play,
        RipDetails::Gbs {
            stack_pointer: word(bytes, 0x0c),
            timer_modulo: modulo,
            timer_control: control,
            double_speed,
            initial_play_rate_hz,
            logical_page_count: page_count,
            leading_padding: u32::from(load),
            page_size: 0x4000,
        },
    ))
}

fn nsf(bytes: &[u8], warnings: &mut Vec<RipWarning>) -> Result<Header, MalformedInput> {
    let load = word(bytes, 8);
    let init = word(bytes, 10);
    let play = word(bytes, 12);
    let expansion = bytes[0x7b];
    let fds = expansion & 4 != 0;
    if [load, init, play]
        .iter()
        .any(|&address| address < if fds { 0x6000 } else { 0x8000 })
    {
        return Err(MalformedInput::InvalidAddress);
    }
    if fds {
        warnings.push(RipWarning::FdsMapping);
        for (entry, cpu_address) in [("load", load), ("init", init), ("play", play)] {
            if cpu_address < 0x8000 {
                warnings.push(RipWarning::LowFdsAddress { entry, cpu_address });
            }
        }
    }
    let declared = u32::from_le_bytes([bytes[0x7d], bytes[0x7e], bytes[0x7f], 0]);
    let available = (bytes.len() - 0x80) as u32;
    let program_len = if declared == 0 { available } else { declared };
    if program_len > available {
        return Err(MalformedInput::ProgramLengthExceedsSource);
    }
    let program = FileSpan {
        offset: 0x80,
        byte_len: program_len,
    };
    let opaque_metadata = (program_len < available).then_some(FileSpan {
        offset: 0x80 + program_len,
        byte_len: available - program_len,
    });
    if let Some(span) = opaque_metadata {
        warnings.push(RipWarning::OpaqueMetadata {
            byte_len: span.byte_len,
        });
    }
    if bytes[0x7c] != 0 {
        warnings.push(RipWarning::Nsf2FeatureFlags { value: bytes[0x7c] });
    }
    let region_bits = bytes[0x7a];
    if region_bits & 0xfc != 0 {
        warnings.push(RipWarning::ReservedBits {
            field: "region",
            value: region_bits & 0xfc,
        });
    }
    if expansion & 0x80 != 0 {
        warnings.push(RipWarning::ReservedBits {
            field: "expansion",
            value: 0x80,
        });
    }
    let region = match region_bits & 3 {
        0 => NsfRegion::Ntsc,
        1 => NsfRegion::Pal,
        2 => NsfRegion::DualNtscPreferred,
        _ => NsfRegion::DualPalPreferred,
    };
    let ntsc_period_us = word(bytes, 0x6e);
    let pal_period_us = word(bytes, 0x78);
    if ntsc_period_us == 0 && region != NsfRegion::Pal {
        warnings.push(RipWarning::UnspecifiedPeriod { region: "ntsc" });
    }
    if pal_period_us == 0 && region != NsfRegion::Ntsc {
        warnings.push(RipWarning::UnspecifiedPeriod { region: "pal" });
    }
    let expansion_chips: Vec<_> = [
        "vrc6",
        "vrc7",
        "fds",
        "mmc5",
        "namco163",
        "sunsoft5b",
        "vt02_plus",
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(bit, chip)| (expansion & (1 << bit) != 0).then_some(chip))
    .collect();
    if expansion_chips.len() > 1 {
        warnings.push(RipWarning::MultipleExpansionChips);
    }
    if expansion & 0x40 != 0 {
        warnings.push(RipWarning::ExpansionCompatibility { chip: "vt02_plus" });
    }
    let banks: [u8; 8] = bytes[0x70..0x78]
        .try_into()
        .expect("validated NSF header width");
    let banking = banks.iter().any(|&bank| bank != 0);
    let padding = if banking { u32::from(load & 0xfff) } else { 0 };
    let bank_count = banking.then_some((padding + program_len).div_ceil(0x1000));
    if let Some(pages) = bank_count {
        if pages > 256 {
            warnings.push(RipWarning::PageIndexExceedsByte { pages });
        }
        for (slot, &bank) in banks.iter().enumerate() {
            if u32::from(bank) >= pages {
                warnings.push(RipWarning::InitialBankBeyondProgram {
                    slot: slot as u8,
                    bank,
                    pages,
                });
            }
        }
    } else if u32::from(load) + program_len > 0x1_0000 {
        warnings.push(RipWarning::ProgramExceedsLinearMemory {
            byte_len: program_len,
        });
    }
    let source_offset = |address: u16| {
        let relative = if banking {
            let slot = if address < 0x8000 {
                usize::from((address >> 12) - 6) + 6
            } else {
                usize::from((address >> 12) - 8)
            };
            (u32::from(banks[slot]) * 0x1000 + u32::from(address & 0xfff)).checked_sub(padding)
        } else {
            u32::from(address).checked_sub(u32::from(load))
        };
        relative
            .filter(|&offset| offset < program_len)
            .map(|offset| program.offset + offset)
    };
    let init = entry(init, source_offset(init), "init", warnings);
    let play = entry(play, source_offset(play), "play", warnings);
    Ok((
        program,
        opaque_metadata,
        load,
        init,
        play,
        RipDetails::Nsf {
            ntsc_period_us,
            pal_period_us,
            region_bits,
            region,
            expansion_bits: expansion,
            expansion_chips,
            initial_banks: banks,
            banking_enabled: banking,
            bank_count,
            leading_padding: padding,
            bank_size: 0x1000,
            nsf2_flags: bytes[0x7c],
            declared_program_bytes: declared,
        },
    ))
}
