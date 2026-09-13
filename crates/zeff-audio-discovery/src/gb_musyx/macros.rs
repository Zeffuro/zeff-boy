use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    Budget, ReadError,
    project::{Project, be, byte},
    require,
};

const SIZES: [usize; 42] = [
    1, 4, 6, 6, 5, 4, 3, 3, 4, 2, 2, 5, 2, 1, 2, 2, 2, 3, 2, 2, 3, 4, 4, 4, 3, 1, 2, 5, 0, 5, 5, 1,
    1, 2, 1, 1, 2, 1, 3, 3, 2, 3,
];
type Node = (usize, u8);

pub(super) fn inspect(
    project: &Project<'_>,
    roots: BTreeSet<Node>,
    ranges: &mut Vec<(usize, usize)>,
    budget: &mut Budget<'_>,
) -> Result<(), ReadError> {
    let bytes = project.bytes;
    let mut todo: Vec<_> = roots.into_iter().collect();
    let mut nodes = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut samples = BTreeSet::new();
    let mut adsrs: [BTreeSet<usize>; 4] = std::array::from_fn(|_| BTreeSet::new());
    while let Some((at, voice)) = todo.pop() {
        budget.charge()?;
        if nodes.contains_key(&(at, voice)) {
            continue;
        }
        require(
            at >= project.macro_table + project.macros.len() * 2 && at < project.adsr_table,
            "macro control flow leaves its data region",
        )?;
        let op = byte(bytes, at)?;
        let size = *SIZES
            .get(usize::from(op))
            .ok_or(ReadError::Invalid("unknown macro opcode"))?;
        require(
            size != 0 && at + size <= project.adsr_table,
            "invalid macro instruction",
        )?;
        for pos in at..at + size {
            require(
                tags.insert(pos, at).is_none_or(|old| old == at),
                "macro branch enters an operand",
            )?;
        }
        ranges.push((at, at + size));
        let mut edges = Vec::new();
        if !matches!(op, 0x00 | 0x23 | 0x06) {
            edges.push((at + size, voice));
        }
        if matches!(op, 0x06 | 0x18) {
            edges.push((project.macro_table + be(bytes, at + 1)?, voice));
        }
        if matches!(op, 0x15..=0x17) {
            edges.push((project.macro_table + be(bytes, at + 2)?, voice));
        }
        if op == 0x05 {
            let displacement = be(bytes, at + 2)? as u16 as i16;
            let target = (at + size)
                .checked_add_signed(isize::from(displacement))
                .ok_or(ReadError::Invalid("invalid macro relative branch"))?;
            edges.push((target, voice));
        }
        if op == 0x26 {
            edges.extend(project.roots(byte(bytes, at + 2)?, byte(bytes, at + 1)? & 127)?);
        }
        if op == 0x0f {
            let index = usize::from(byte(bytes, at + 1)?);
            let data = *project
                .adsrs
                .get(index)
                .ok_or(ReadError::Invalid("invalid ADSR selector"))?;
            ranges.push((data, data + 7));
            adsrs[usize::from(voice)].insert(data);
        }
        if voice == 2 && (op == 0x21 || (op == 0x0c && byte(bytes, at + 1)? != 255)) {
            let index = usize::from(byte(bytes, at + 1)?);
            require(index < project.samples.len(), "invalid sample selector")?;
            samples.insert(index);
        }
        if voice == 2 && op == 0x22 {
            if project.sample_map.is_empty() {
                samples.extend(0..project.samples.len());
            } else {
                samples.extend(project.sample_map.iter().copied().map(usize::from));
            }
        }
        match op {
            0x02 => require(
                byte(bytes, at + 1)? & 0x7e == 0,
                "invalid portamento voice flags",
            )?,
            0x03 => require(
                byte(bytes, at + 5)? & 127 == 0,
                "invalid pitch-sweep voice flags",
            )?,
            0x0a => require(byte(bytes, at + 1)? <= 2, "invalid panning selector")?,
            0x12 => require(
                matches!(byte(bytes, at + 1)?, 255) || byte(bytes, at + 1)? == voice,
                "cross-voice keyoff requires unresolved trap state",
            )?,
            0x1a => require(byte(bytes, at + 1)? < 8, "invalid user flag")?,
            0x1d | 0x1e => require(
                byte(bytes, at + 1)? <= 15 && byte(bytes, at + 2)? <= 15,
                "invalid pulse width limits",
            )?,
            0x24 => require(byte(bytes, at + 1)? <= 15, "invalid fixed pulse width")?,
            _ => (),
        }
        todo.extend(edges.iter().copied());
        nodes.insert((at, voice), (op, edges));
    }
    // Only WAIT suspends the native macro dispatcher; all other cycles must be finite here.
    let mut indegree: BTreeMap<_, usize> = nodes.keys().map(|node| (*node, 0)).collect();
    for (op, edges) in nodes.values() {
        budget.charge()?;
        if *op != 4 {
            for edge in edges {
                *indegree
                    .get_mut(edge)
                    .ok_or(ReadError::Invalid("unmapped macro edge"))? += 1;
            }
        }
    }
    let mut ready: VecDeque<_> = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(*node))
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop_front() {
        budget.charge()?;
        visited += 1;
        let (op, edges) = &nodes[&node];
        if *op != 4 {
            for edge in edges {
                let count = indegree
                    .get_mut(edge)
                    .ok_or(ReadError::Invalid("unmapped macro edge"))?;
                *count -= 1;
                if *count == 0 {
                    ready.push_back(*edge);
                }
            }
        }
    }
    require(visited == nodes.len(), "macro cycle has no qualified yield")?;
    keyoff_aliases(project, &adsrs, ranges, budget)?;
    for index in samples {
        ranges.push(project.samples[index]);
    }
    Ok(())
}

fn keyoff_aliases(
    project: &Project<'_>,
    adsrs: &[BTreeSet<usize>; 4],
    ranges: &mut Vec<(usize, usize)>,
    budget: &mut Budget<'_>,
) -> Result<(), ReadError> {
    let pointers: [BTreeSet<u16>; 4] = std::array::from_fn(|voice| {
        std::iter::once(0)
            .chain(adsrs[voice].iter().flat_map(|&at| {
                let cpu = 0x4000 + (at % 0x4000) as u16;
                [cpu + 2, cpu + 4]
            }))
            .collect()
    });
    // Native keyoff uses the voice as a byte offset into a two-byte pointer array.
    for voice in 1..4 {
        if adsrs[voice].is_empty() {
            continue;
        }
        let low: BTreeSet<_> = pointers[voice / 2]
            .iter()
            .map(|pointer| pointer.to_le_bytes()[voice % 2])
            .collect();
        let high: BTreeSet<_> = pointers[voice.div_ceil(2)]
            .iter()
            .map(|pointer| pointer.to_le_bytes()[(voice + 1) % 2])
            .collect();
        for lo in &low {
            for hi in &high {
                budget.charge()?;
                let pointer = usize::from(u16::from_le_bytes([*lo, *hi]));
                for field in [1, 3] {
                    let cpu = pointer + field;
                    require(
                        cpu + 2 <= 0x8000 && cpu / 0x4000 == (cpu + 1) / 0x4000,
                        "ADSR keyoff alias leaves a ROM window",
                    )?;
                    let at = if cpu < 0x4000 {
                        cpu
                    } else {
                        usize::from(project.driver.bank) * 0x4000 + cpu - 0x4000
                    };
                    require(
                        ![(0x40, 0x43), (0x50, 0x53), (0x100, 0x103), (0x150, 0x280)]
                            .iter()
                            .any(|&(start, end)| at < end && at + 2 > start),
                        "ADSR keyoff alias overlaps the native handoff",
                    )?;
                    require(
                        at + 2 <= project.bytes.len(),
                        "ADSR keyoff alias leaves the source",
                    )?;
                    ranges.push((at, at + 2));
                }
            }
        }
    }
    Ok(())
}
