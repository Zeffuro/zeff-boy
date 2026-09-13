use super::super::word;
use super::{Budget, RamCopy, RomSpan, ScanStop, driver, half, signatures as sig};

type Setup = (Vec<RamCopy>, Vec<RomSpan>);

pub(super) fn inspect(
    bytes: &[u8],
    row: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<Setup>, ScanStop> {
    let mut copies = Vec::new();
    let mut spans = Vec::new();
    for pc in (0xc0..bytes.len().min(0x400)).step_by(2) {
        budget.charge()?;
        if let Some((copy, evidence)) = thumb_copy(bytes, pc) {
            copies.push(copy);
            spans.extend(evidence);
        }
    }
    if copies.is_empty() {
        for pc in (0xc0..bytes.len().min(0x400)).step_by(4) {
            budget.charge()?;
            if let Some((copy, evidence)) = arm_copy(bytes, pc) {
                copies.push(copy);
                spans.extend(evidence);
            }
        }
    }
    if copies.is_empty() {
        for pc in (0..row).step_by(2) {
            budget.charge()?;
            if let Some((pair, evidence)) = paired_byte_copy(bytes, pc) {
                copies.extend(pair);
                spans.extend(evidence);
            }
            if copies.len() > 4 {
                return Ok(None);
            }
        }
    }
    if copies.len() > 4
        || !copies.iter().any(|copy| copy.destination == 0x0300_0000)
        || !copies.iter().any(|copy| copy.destination == 0x0200_0000)
    {
        return Ok(None);
    }
    for (index, first) in copies.iter().enumerate() {
        for second in &copies[index + 1..] {
            budget.charge()?;
            let start = first.destination.max(second.destination);
            let end = (first.destination + first.source.byte_len)
                .min(second.destination + second.source.byte_len);
            if start < end {
                let left = (first.source.effective_offset + start - first.destination) as usize;
                let right = (second.source.effective_offset + start - second.destination) as usize;
                let len = (end - start) as usize;
                if bytes[left..left + len] != bytes[right..right + len] {
                    return Ok(None);
                }
            }
        }
    }
    super::merge_spans(&mut spans);
    Ok(Some((copies, spans)))
}

fn valid_copy(bytes: &[u8], source: u32, destination: u32, size: u32) -> Option<RamCopy> {
    let end = destination.checked_add(size)?;
    if size == 0
        || size > 0x40000
        || !size.is_multiple_of(4)
        || !destination.is_multiple_of(4)
        || !((0x0200_0000..=0x0204_0000).contains(&destination) && end <= 0x0204_0000
            || (0x0300_0000..=0x0300_7f00).contains(&destination) && end <= 0x0300_7f00)
    {
        return None;
    }
    let offset = super::super::rom_pointer(bytes, source, size as usize, 4)?;
    Some(RamCopy {
        source: RomSpan::new(offset, size as usize),
        destination,
    })
}

fn thumb_copy(bytes: &[u8], pc: usize) -> Option<(RamCopy, Vec<RomSpan>)> {
    if half(bytes, pc)? & 0xff00 != 0x4900
        || half(bytes, pc + 2)? & 0xff00 != 0x4a00
        || half(bytes, pc + 4)? & 0xff00 != 0x4c00
    {
        return None;
    }
    let helper = driver::thumb_branch(bytes, pc + 6)?;
    let signature = if driver::exact(bytes, helper, sig::COPY_THUMB) {
        sig::COPY_THUMB
    } else if driver::exact(bytes, helper, sig::COPY_THUMB_CHECKED) {
        sig::COPY_THUMB_CHECKED
    } else {
        return None;
    };
    let source = driver::literal(bytes, pc)?;
    let destination = driver::literal(bytes, pc + 2)?;
    let end = driver::literal(bytes, pc + 4)?;
    let copy = valid_copy(bytes, source, destination, end.checked_sub(destination)?)?;
    let mut spans = vec![RomSpan::new(pc, 10), RomSpan::new(helper, signature.len())];
    for offset in [pc, pc + 2, pc + 4] {
        spans.push(RomSpan::new(driver::literal_slot(bytes, offset)?, 4));
    }
    Some((copy, spans))
}

fn arm_copy(bytes: &[u8], pc: usize) -> Option<(RamCopy, Vec<RomSpan>)> {
    let mut values = [0; 3];
    let mut spans = vec![RomSpan::new(pc, 28)];
    for (register, value) in values.iter_mut().enumerate() {
        let instruction = pc + 4 * register;
        let opcode = word(bytes, instruction)?;
        if opcode & 0xffff_f000 != 0xe59f_0000 + (register as u32) * 0x1000 {
            return None;
        }
        let slot = instruction + 8 + (opcode & 0xfff) as usize;
        *value = word(bytes, slot)?;
        spans.push(RomSpan::new(slot, 4));
    }
    if !driver::exact(
        bytes,
        pc + 12,
        &[
            0x04, 0x30, 0x92, 0xe4, 0x04, 0x30, 0x80, 0xe4, 0x01, 0x00, 0x50, 0xe1, 0xfb, 0xff,
            0xff, 0xba,
        ],
    ) {
        return None;
    }
    let copy = valid_copy(
        bytes,
        values[2],
        values[0],
        values[1].checked_sub(values[0])?,
    )?;
    Some((copy, spans))
}

fn paired_byte_copy(bytes: &[u8], pc: usize) -> Option<([RamCopy; 2], Vec<RomSpan>)> {
    for (offset, register) in [(0, 0), (2, 1), (4, 2), (6, 4), (14, 0), (16, 1), (18, 2)] {
        if half(bytes, pc + offset)? & 0xff00 != 0x4800 + register * 0x100 {
            return None;
        }
    }
    if half(bytes, pc + 8)? != 0x4022
        || half(bytes, pc + 20)? != 0x4022
        || driver::literal(bytes, pc + 6)? != 0x00ff_ffff
    {
        return None;
    }
    let helper = driver::thumb_branch(bytes, pc + 10)?;
    if driver::thumb_branch(bytes, pc + 22)? != helper
        || !driver::exact(bytes, helper, sig::COPY_BYTES)
    {
        return None;
    }
    let copy = |offset| {
        valid_copy(
            bytes,
            driver::literal(bytes, offset + 2)?,
            driver::literal(bytes, offset)?,
            driver::literal(bytes, offset + 4)? & 0x00ff_ffff,
        )
    };
    let copies = [copy(pc)?, copy(pc + 14)?];
    let mut spans = vec![
        RomSpan::new(pc, 26),
        RomSpan::new(helper, sig::COPY_BYTES.len()),
    ];
    for offset in [0, 2, 4, 6, 14, 16, 18] {
        spans.push(RomSpan::new(driver::literal_slot(bytes, pc + offset)?, 4));
    }
    Some((copies, spans))
}
