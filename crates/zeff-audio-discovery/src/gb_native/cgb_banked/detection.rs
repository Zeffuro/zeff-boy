use crate::{
    Budget, ScanStop,
    drivers::{
        BankedDriverParameters, DriverFinding, DriverFingerprint, DriverTableRow,
        DriverTableRowState, Qualification,
    },
};

use super::{candidate, profiles, recipe::RECIPE, rom_span};

const SELECTOR_LEN: usize = 68;
const MUSIC_TABLE_OPERAND_OFFSET: u16 = 0x13;
const TICK_DRIVER_OFFSET: u16 = 32;
const TICK_DRIVER_BRANCH_OFFSET: u16 = 40;

pub(in crate::gb_native) fn scan(
    bytes: &[u8],
    source_sha256: Option<&str>,
    findings: &mut Vec<DriverFinding>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    budget.charge()?;
    if !candidate(bytes) {
        return Ok(());
    }
    scan_variants(
        bytes,
        source_sha256,
        findings,
        budget,
        max_candidates,
        profiles::all(),
    )
}

pub(super) fn scan_variants(
    bytes: &[u8],
    source_sha256: Option<&str>,
    findings: &mut Vec<DriverFinding>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    variants: impl IntoIterator<Item = &'static super::recipe::DriverVariant>,
) -> Result<(), ScanStop> {
    for variant in variants {
        budget.charge()?;
        let Some(driver_sha256) = variant.driver_sha256 else {
            continue;
        };
        if !matches_driver(bytes, variant.parameters, driver_sha256, budget)? {
            continue;
        }
        if findings.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        let finding = inspect(bytes, variant, driver_sha256, source_sha256, budget)?;
        findings.push(finding);
    }
    Ok(())
}

fn matches_driver(
    bytes: &[u8],
    parameters: super::recipe::VariantParameters,
    driver_sha256: &str,
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    if parameters.cartridge_type != bytes.get(0x147).copied().unwrap_or_default() {
        return Ok(false);
    }
    let Some(driver_len) = parameters.driver_end.checked_sub(parameters.driver) else {
        return Ok(false);
    };
    if driver_len == 0
        || parameters.driver_end > 0x4000
        || !within_driver(
            parameters.init,
            usize::from(parameters.init_len),
            parameters.driver,
            parameters.driver_end,
        )
        || !within_driver(
            parameters.selector,
            SELECTOR_LEN,
            parameters.driver,
            parameters.driver_end,
        )
        || !within_driver(
            parameters.tick,
            RECIPE.tick_len,
            parameters.driver,
            parameters.driver_end,
        )
        || !valid_table(parameters)
    {
        return Ok(false);
    }
    let driver_start = usize::from(parameters.driver);
    let driver_end = usize::from(parameters.driver_end);
    let Some(driver) = bytes.get(driver_start..driver_end) else {
        return Ok(false);
    };
    for _ in driver.chunks(256) {
        budget.charge()?;
    }
    if zeff_firmware::sha256_hex(driver) != driver_sha256 {
        return Ok(false);
    }
    let Some(music_table_operand) = word_at(bytes, parameters.driver, MUSIC_TABLE_OPERAND_OFFSET)
    else {
        return Ok(false);
    };
    let music_table_opcode = parameters
        .driver
        .checked_add(MUSIC_TABLE_OPERAND_OFFSET - 1)
        .and_then(|address| bytes.get(usize::from(address)))
        .copied();
    let music_table_steps = parameters
        .driver
        .checked_add(MUSIC_TABLE_OPERAND_OFFSET + 2)
        .and_then(|address| bytes.get(usize::from(address)..usize::from(address) + 3));
    Ok(music_table_opcode == Some(0x21)
        && music_table_operand == parameters.table
        && music_table_steps == Some(&[0x09, 0x09, 0x09])
        && word_at(bytes, parameters.tick, TICK_DRIVER_OFFSET) == Some(parameters.driver)
        && word_at(bytes, parameters.tick, TICK_DRIVER_BRANCH_OFFSET)
            == parameters.driver.checked_add(0x1b))
}

fn inspect(
    bytes: &[u8],
    variant: &'static super::recipe::DriverVariant,
    driver_sha256: &str,
    source_sha256: Option<&str>,
    budget: &mut Budget<'_>,
) -> Result<DriverFinding, ScanStop> {
    let parameters = variant.parameters;
    let init = rom_span(bytes, 0, parameters.init, usize::from(parameters.init_len))
        .ok_or(ScanStop::ValidationLimit)?;
    let selector =
        rom_span(bytes, 0, parameters.selector, SELECTOR_LEN).ok_or(ScanStop::ValidationLimit)?;
    let tick =
        rom_span(bytes, 0, parameters.tick, RECIPE.tick_len).ok_or(ScanStop::ValidationLimit)?;
    let fingerprint = rom_span(
        bytes,
        0,
        parameters.driver,
        usize::from(
            parameters
                .driver_end
                .checked_sub(parameters.driver)
                .ok_or(ScanStop::ValidationLimit)?,
        ),
    )
    .ok_or(ScanStop::ValidationLimit)?;
    let table = rom_span(
        bytes,
        0,
        parameters.table,
        usize::from(
            parameters
                .table_rows
                .checked_mul(3)
                .ok_or(ScanStop::ValidationLimit)?,
        ),
    )
    .ok_or(ScanStop::ValidationLimit)?;
    let mut rows = Vec::with_capacity(usize::from(parameters.table_rows));
    for row in 0..parameters.table_rows {
        budget.charge()?;
        rows.push(inspect_row(
            bytes,
            parameters.table,
            RECIPE.selector_base,
            row,
        )?);
    }
    let qualification = source_sha256
        .filter(|source| variant.qualification.sources.contains(source))
        .map_or(Qualification::Candidate, |_| Qualification::KnownRom {
            profile: variant.id,
            native_playback_selectors: variant
                .qualification
                .cues
                .iter()
                .map(|cue| cue.raw)
                .collect(),
        });
    Ok(DriverFinding {
        family: RECIPE.id,
        variant: variant.id,
        qualification,
        fingerprint: DriverFingerprint {
            span: fingerprint,
            sha256: driver_sha256.to_owned(),
        },
        parameters: BankedDriverParameters {
            cartridge_type: parameters.cartridge_type,
            init,
            selector,
            tick,
            table,
            selector_base: RECIPE.selector_base,
            inspected_rows: parameters.table_rows,
        },
        rows,
    })
}

fn inspect_row(
    bytes: &[u8],
    table: u16,
    selector_base: u8,
    row: u16,
) -> Result<DriverTableRow, ScanStop> {
    let table_address = table
        .checked_add(row.checked_mul(3).ok_or(ScanStop::ValidationLimit)?)
        .ok_or(ScanStop::ValidationLimit)?;
    let table_entry = rom_span(bytes, 0, table_address, 3).ok_or(ScanStop::ValidationLimit)?;
    let offset = table_entry.effective_offset as usize;
    let bank = bytes[offset];
    let address = u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]);
    let raw_selector = u8::try_from(
        u16::from(selector_base)
            .checked_add(row)
            .ok_or(ScanStop::ValidationLimit)?,
    )
    .map_err(|_| ScanStop::ValidationLimit)?;
    let Some(header_byte) = rom_span(bytes, bank, address, 1) else {
        return Ok(DriverTableRow {
            raw_selector,
            entry: table_entry,
            state: DriverTableRowState::UnmappedHeader,
            header: None,
            channels: Vec::new(),
        });
    };
    let mask = bytes[header_byte.effective_offset as usize];
    if mask == 0 {
        return Ok(DriverTableRow {
            raw_selector,
            entry: table_entry,
            state: DriverTableRowState::EmptyChannelMask,
            header: Some(header_byte),
            channels: Vec::new(),
        });
    }
    if mask & !0x0f != 0 {
        return Ok(DriverTableRow {
            raw_selector,
            entry: table_entry,
            state: DriverTableRowState::InvalidChannelMask,
            header: Some(header_byte),
            channels: Vec::new(),
        });
    }
    let Some(header) = rom_span(bytes, bank, address, 1 + mask.count_ones() as usize * 2) else {
        return Ok(DriverTableRow {
            raw_selector,
            entry: table_entry,
            state: DriverTableRowState::UnmappedHeader,
            header: None,
            channels: Vec::new(),
        });
    };
    let mut channels = Vec::with_capacity(mask.count_ones() as usize);
    for number in 0..4 {
        if mask & (1 << number) == 0 {
            continue;
        }
        let channel_offset = u16::try_from(channels.len())
            .ok()
            .and_then(|count| count.checked_mul(2))
            .and_then(|offset| address.checked_add(1 + offset))
            .ok_or(ScanStop::ValidationLimit)?;
        let entry = rom_span(bytes, bank, channel_offset, 2).ok_or(ScanStop::ValidationLimit)?;
        let offset = entry.effective_offset as usize;
        let pointer = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let Some(sequence) = rom_span(bytes, bank, pointer, 1) else {
            return Ok(DriverTableRow {
                raw_selector,
                entry: table_entry,
                state: DriverTableRowState::UnmappedChannel,
                header: Some(header),
                channels,
            });
        };
        channels.push(crate::gb_native::GbNativeChannel {
            number: number + 1,
            entry,
            sequence,
        });
    }
    Ok(DriverTableRow {
        raw_selector,
        entry: table_entry,
        state: DriverTableRowState::MappedChannelHeader,
        header: Some(header),
        channels,
    })
}

fn within_driver(address: u16, len: usize, driver: u16, driver_end: u16) -> bool {
    let Some(end) = usize::from(address).checked_add(len) else {
        return false;
    };
    address >= driver && end <= usize::from(driver_end) && end <= 0x4000
}

fn valid_table(parameters: super::recipe::VariantParameters) -> bool {
    let Some(len) = usize::from(parameters.table_rows).checked_mul(3) else {
        return false;
    };
    len != 0
        && usize::from(parameters.table)
            .checked_add(len)
            .is_some_and(|end| end <= 0x4000)
        && u16::from(RECIPE.selector_base)
            .checked_add(parameters.table_rows)
            .is_some_and(|end| end <= u16::from(u8::MAX) + 1)
}

fn word_at(bytes: &[u8], address: u16, offset: u16) -> Option<u16> {
    let offset = usize::from(address.checked_add(offset)?);
    bytes
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}
