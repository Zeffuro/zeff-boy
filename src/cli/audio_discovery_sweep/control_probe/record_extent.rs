use serde_json::{Value, json};

use super::super::nes_source::Nrom;

const MAX_ROWS: usize = 4096;

/// Finds fully observed, fixed-size APU records copied by one narrow 6502 loop.
pub(super) fn analyze(source: &[u8], control: &Value, boundaries: &[u64]) -> Value {
    let Some(nrom) = Nrom::parse(source) else {
        return json!([]);
    };
    let Some(reads) = control["entry_argument_reads"].as_array() else {
        return json!([]);
    };
    let Some(observations) = control["observations"].as_array() else {
        return json!([]);
    };
    if reads.len() > MAX_ROWS || observations.len() > MAX_ROWS {
        return json!([]);
    }

    let mut extents = Vec::new();
    for (entry_argument_read_index, read) in reads.iter().enumerate() {
        let Some(anchor) = Anchor::parse(read) else {
            continue;
        };
        let Some(contract) = Contract::parse(source, nrom, &anchor) else {
            continue;
        };
        if !anchor_matches(read, &anchor, &contract) {
            continue;
        }
        let Some(extent) = observed_extent(
            source,
            observations,
            entry_argument_read_index,
            &anchor,
            &contract,
            boundaries,
        ) else {
            continue;
        };
        extents.push(extent);
    }
    json!(extents)
}

#[derive(Clone, Copy)]
struct Anchor {
    entry_pc: u16,
    entry_cycle: u64,
    load_pc: u16,
    index: u8,
    event_index: u64,
    read_cycle: u64,
}

impl Anchor {
    fn parse(read: &Value) -> Option<Self> {
        Some(Self {
            entry_pc: u16_value(&read["entry"]["pc"])?,
            entry_cycle: u64_value(&read["entry"]["cpu_cycle"])?,
            load_pc: u16_value(&read["rom_read"]["pc"])?,
            index: u8_value(&read["rom_read"]["index_value"])?,
            event_index: u64_value(&read["event_index"])?,
            read_cycle: u64_value(&read["rom_read"]["cpu_cycle"])?,
        })
    }
}

struct Contract {
    ldx_pc: u16,
    load_pc: u16,
    store_pc: u16,
    iny_pc: u16,
    inx_pc: u16,
    cpx_pc: u16,
    bne_pc: u16,
    table: u16,
    address: u16,
    start: u64,
    destination: u16,
    length: u8,
    ldx_source_offset: u64,
    load_source_offset: u64,
    values: Vec<u8>,
    ldx: Vec<u8>,
    load: Vec<u8>,
    store: Vec<u8>,
    iny: Vec<u8>,
    inx: Vec<u8>,
    cpx: Vec<u8>,
    bne: Vec<u8>,
}

impl Contract {
    fn parse(source: &[u8], nrom: Nrom, anchor: &Anchor) -> Option<Self> {
        let ldx_pc = anchor.load_pc.checked_sub(2)?;
        let store_pc = anchor.load_pc.checked_add(3)?;
        let iny_pc = anchor.load_pc.checked_add(6)?;
        let inx_pc = anchor.load_pc.checked_add(7)?;
        let cpx_pc = anchor.load_pc.checked_add(8)?;
        let bne_pc = anchor.load_pc.checked_add(10)?;
        let ldx = bytes_at(source, nrom, ldx_pc, 2)?;
        let load = bytes_at(source, nrom, anchor.load_pc, 3)?;
        let store = bytes_at(source, nrom, store_pc, 3)?;
        let iny = bytes_at(source, nrom, iny_pc, 1)?;
        let inx = bytes_at(source, nrom, inx_pc, 1)?;
        let cpx = bytes_at(source, nrom, cpx_pc, 2)?;
        let bne = bytes_at(source, nrom, bne_pc, 2)?;
        bytes_at(source, nrom, ldx_pc, 14)?;
        let ldx_source_offset = nrom.offset_for(ldx_pc)?;
        let load_source_offset = nrom.offset_for(anchor.load_pc)?;
        if ldx != [0xa2, 0x00]
            || load.first() != Some(&0xb9)
            || store.first() != Some(&0x9d)
            || iny != [0xc8]
            || inx != [0xe8]
            || cpx.first() != Some(&0xe0)
            || bne.first() != Some(&0xd0)
        {
            return None;
        }
        let length = *cpx.get(1)?;
        if !(1..=16).contains(&length) {
            return None;
        }
        let target = i32::from(bne_pc)
            .checked_add(2)?
            .checked_add(i32::from(*bne.get(1)? as i8))?;
        if target != i32::from(anchor.load_pc) {
            return None;
        }
        let table = u16::from_le_bytes([*load.get(1)?, *load.get(2)?]);
        let destination = u16::from_le_bytes([*store.get(1)?, *store.get(2)?]);
        let last_index = anchor.index.checked_add(length.checked_sub(1)?)?;
        let address = table.checked_add(u16::from(anchor.index))?;
        let last_address = table.checked_add(u16::from(last_index))?;
        let last_destination = destination.checked_add(u16::from(length.checked_sub(1)?))?;
        if !(0x8000..=0xffff).contains(&table)
            || !(0x8000..=0xffff).contains(&address)
            || !(0x8000..=0xffff).contains(&last_address)
            || !apu_register(destination)
            || !apu_register(last_destination)
            || !(0..length).all(|offset| {
                destination
                    .checked_add(u16::from(offset))
                    .is_some_and(apu_register)
            })
        {
            return None;
        }
        let values = bytes_at(source, nrom, address, usize::from(length))?;
        let start = nrom.offset_for(address)?;
        if values.len() != usize::from(length) {
            return None;
        }
        Some(Self {
            ldx_pc,
            load_pc: anchor.load_pc,
            store_pc,
            iny_pc,
            inx_pc,
            cpx_pc,
            bne_pc,
            table,
            address,
            start,
            destination,
            length,
            ldx_source_offset,
            load_source_offset,
            values,
            ldx,
            load,
            store,
            iny,
            inx,
            cpx,
            bne,
        })
    }
}

fn observed_extent(
    source: &[u8],
    observations: &[Value],
    entry_argument_read_index: usize,
    anchor: &Anchor,
    contract: &Contract,
    boundaries: &[u64],
) -> Option<Value> {
    let first_observation =
        unique_observation(observations, anchor.event_index, anchor, contract, 0)?;
    let mut event_indices = Vec::with_capacity(usize::from(contract.length));
    let mut previous_cycle = None;
    for offset in 0..contract.length {
        let observation = observations.get(first_observation.checked_add(usize::from(offset))?)?;
        let event_index = u64_value(&observation["event_index"])?;
        if event_index != anchor.event_index.checked_add(u64::from(offset))?
            || (offset != 0 && event_index <= *event_indices.last()?)
        {
            return None;
        }
        let cycle = u64_value(&observation["cycle"])?;
        if previous_cycle.is_some_and(|previous| cycle <= previous) {
            return None;
        }
        if !observation_matches(observation, event_index, anchor, contract, offset) {
            return None;
        }
        event_indices.push(event_index);
        previous_cycle = Some(cycle);
    }
    let last = observations.get(first_observation + usize::from(contract.length) - 1)?;
    let last_cycle = u64_value(&last["writer_before"]["cycle"])?;
    if last_cycle < anchor.read_cycle
        || boundaries
            .iter()
            .any(|cycle| (anchor.read_cycle..=last_cycle).contains(cycle))
    {
        return None;
    }
    if observations
        .iter()
        .filter(|observation| same_entry_store(observation, anchor, contract))
        .count()
        != usize::from(contract.length)
    {
        return None;
    }
    let values = (0..contract.length)
        .map(|offset| source_value(source, contract.start.checked_add(u64::from(offset))?))
        .collect::<Option<Vec<_>>>()?;
    Some(json!({
        "entry": {"pc": anchor.entry_pc, "cpu_cycle": anchor.entry_cycle},
        "entry_argument_read_index": entry_argument_read_index,
        "event_indices": event_indices,
        "execution_window": {"first_load_cpu_cycle": anchor.read_cycle,
            "last_store_cpu_cycle": last_cycle, "discontinuity_free": true},
        "source": {
            "table": contract.table,
            "address": contract.address,
            "start": contract.start,
            "length": contract.length,
            "values": const_hex::encode(values),
        },
        "instruction_contract": {
            "ldx_immediate": instruction(contract.ldx_pc, &contract.ldx),
            "lda_absolute_y": instruction(contract.load_pc, &contract.load),
            "sta_absolute_x": json!({
                "pc": contract.store_pc, "bytes": const_hex::encode(&contract.store),
                "base_address": contract.destination,
            }),
            "iny": instruction(contract.iny_pc, &contract.iny),
            "inx": instruction(contract.inx_pc, &contract.inx),
            "cpx_immediate": json!({
                "pc": contract.cpx_pc, "bytes": const_hex::encode(&contract.cpx),
                "count": contract.length,
            }),
            "bne": json!({
                "pc": contract.bne_pc, "bytes": const_hex::encode(&contract.bne),
                "target": contract.load_pc,
            }),
        },
    }))
}

fn anchor_matches(read: &Value, anchor: &Anchor, contract: &Contract) -> bool {
    read_matches(read, anchor.event_index, anchor, contract, 0)
        && ordered_initial_witnesses(read, contract)
}

fn unique_observation(
    observations: &[Value],
    event_index: u64,
    anchor: &Anchor,
    contract: &Contract,
    offset: u8,
) -> Option<usize> {
    let matches = observations
        .iter()
        .enumerate()
        .filter(|(_, observation)| {
            observation_matches(observation, event_index, anchor, contract, offset)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Some(*index),
        _ => None,
    }
}

fn same_entry_store(observation: &Value, anchor: &Anchor, contract: &Contract) -> bool {
    u16_value(&observation["pc"]) == Some(contract.store_pc)
        && u16_value(&observation["writer_before"]["pc"]) == Some(contract.store_pc)
        && nearest_entry(observation) == Some((anchor.entry_pc, anchor.entry_cycle))
}

fn ordered_initial_witnesses(read: &Value, contract: &Contract) -> bool {
    let Some(witnesses) = read["witnesses"].as_array() else {
        return false;
    };
    witnesses
        .windows(2)
        .filter(|pair| {
            witness_matches(
                &pair[0],
                contract.ldx_pc,
                contract.ldx_source_offset,
                &contract.ldx,
            ) && witness_matches(
                &pair[1],
                contract.load_pc,
                contract.load_source_offset,
                &contract.load,
            ) && u64_value(&pair[0]["cpu_cycle"])
                .zip(u64_value(&pair[1]["cpu_cycle"]))
                .is_some_and(|(before, after)| before < after)
        })
        .count()
        == 1
}

fn witness_matches(witness: &Value, pc: u16, source_offset: u64, bytes: &[u8]) -> bool {
    u16_value(&witness["pc"]) == Some(pc)
        && u64_value(&witness["source_offset"]) == Some(source_offset)
        && witness["bytes"] == const_hex::encode(bytes)
}

fn read_matches(
    read: &Value,
    event_index: u64,
    anchor: &Anchor,
    contract: &Contract,
    offset: u8,
) -> bool {
    let Some(index) = anchor.index.checked_add(offset) else {
        return false;
    };
    let Some(address) = contract.table.checked_add(u16::from(index)) else {
        return false;
    };
    let Some(source_offset) = contract.start.checked_add(u64::from(offset)) else {
        return false;
    };
    let Some(value) = source_value_from_read(read) else {
        return false;
    };
    u16_value(&read["entry"]["pc"]) == Some(anchor.entry_pc)
        && u64_value(&read["entry"]["cpu_cycle"]) == Some(anchor.entry_cycle)
        && u64_value(&read["event_index"]) == Some(event_index)
        && u16_value(&read["writer_pc"]) == Some(contract.store_pc)
        && u16_value(&read["register"]) == contract.destination.checked_add(u16::from(offset))
        && read["rom_read"]["index_register"] == "y"
        && u8_value(&read["rom_read"]["index_value"]) == Some(index)
        && u16_value(&read["rom_read"]["pc"]) == Some(contract.load_pc)
        && read["rom_read"]["instruction"] == const_hex::encode(&contract.load)
        && u16_value(&read["rom_read"]["address"]) == Some(address)
        && u64_value(&read["rom_read"]["source_offset"]) == Some(source_offset)
        && source_value_from_contract(contract, offset) == Some(value)
}

fn observation_matches(
    observation: &Value,
    event_index: u64,
    anchor: &Anchor,
    contract: &Contract,
    offset: u8,
) -> bool {
    let Some(index) = anchor.index.checked_add(offset) else {
        return false;
    };
    let Some(destination) = contract.destination.checked_add(u16::from(offset)) else {
        return false;
    };
    let Some(value) = source_value_from_contract(contract, offset) else {
        return false;
    };
    u64_value(&observation["event_index"]) == Some(event_index)
        && u16_value(&observation["pc"]) == Some(contract.store_pc)
        && u16_value(&observation["register"]) == Some(destination)
        && u8_value(&observation["value"]) == Some(value)
        && u16_value(&observation["writer_before"]["pc"]) == Some(contract.store_pc)
        && u8_value(&observation["writer_before"]["x"]) == Some(offset)
        && u8_value(&observation["writer_before"]["y"]) == Some(index)
        && u8_value(&observation["writer_before"]["a"]) == Some(value)
        && nearest_entry(observation) == Some((anchor.entry_pc, anchor.entry_cycle))
}

fn source_value_from_read(read: &Value) -> Option<u8> {
    u8_value(&read["rom_read"]["value"])
}

fn source_value_from_contract(contract: &Contract, offset: u8) -> Option<u8> {
    contract.values.get(usize::from(offset)).copied()
}

fn source_value(source: &[u8], offset: u64) -> Option<u8> {
    source.get(usize::try_from(offset).ok()?).copied()
}

fn apu_register(address: u16) -> bool {
    matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
}

fn nearest_entry(observation: &Value) -> Option<(u16, u64)> {
    let frame = observation["call_path"].as_array()?.last()?;
    Some((
        u16_value(&frame["entry"]["pc"])?,
        u64_value(&frame["entry"]["cycle"])?,
    ))
}

fn bytes_at(source: &[u8], nrom: Nrom, address: u16, length: usize) -> Option<Vec<u8>> {
    address.checked_add(u16::try_from(length.checked_sub(1)?).ok()?)?;
    let start = nrom.offset_for(address)?;
    let end = start.checked_add(u64::try_from(length.checked_sub(1)?).ok()?)?;
    for offset in 0..length {
        let cpu = address.checked_add(u16::try_from(offset).ok()?)?;
        if nrom.offset_for(cpu)? != start.checked_add(u64::try_from(offset).ok()?)? {
            return None;
        }
    }
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?.checked_add(1)?;
    source.get(start..end).map(ToOwned::to_owned)
}

fn instruction(pc: u16, bytes: &[u8]) -> Value {
    json!({"pc": pc, "bytes": const_hex::encode(bytes)})
}

fn u64_value(value: &Value) -> Option<u64> {
    value.as_u64()
}

fn u16_value(value: &Value) -> Option<u16> {
    u16::try_from(u64_value(value)?).ok()
}

fn u8_value(value: &Value) -> Option<u8> {
    u8::try_from(u64_value(value)?).ok()
}

#[cfg(test)]
#[path = "record_extent/tests.rs"]
mod tests;
