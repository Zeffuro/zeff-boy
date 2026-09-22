#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop, drivers::CodePointerDisposition};

const ENTRY: u16 = 0x8300;
const TABLE: u16 = 0x9200;

pub fn synthetic_record_rom() -> Vec<u8> {
    record_rom(2, 0, 0x50, 0x0600)
}

pub(super) fn record_rom(banks: u8, relocation: u16, scratch: u8, state: u16) -> Vec<u8> {
    let entry = ENTRY + relocation;
    let table = TABLE + relocation;
    let conditional = entry + 0x44;
    let alternate = entry + 0x80;
    let join = alternate + 13;
    let triple = entry + 0x100;
    let helper = entry + 0x200;
    let mask = helper + 0x40;
    let pointer = scratch + 2;
    let header = scratch + 5;
    let active = state + 0x10;
    let bank = state + 0x70;
    let mut rom = vec![0; 16 + usize::from(banks) * 0x4000];
    rom[..5].copy_from_slice(&[b'N', b'E', b'S', 0x1a, banks]);
    let mut root = Program::new(0x8000);
    root.word(0x20, 0x8100);
    root.word(0x20, entry);
    root.op(0x60);
    root.finish(&mut rom);
    put(&mut rom, 0x8100, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    let mut gate = Program::new(entry);
    gate.byte(0xa6, scratch);
    gate.branch(0xd0, entry + 5);
    gate.op(0x60);
    gate.byte(0xe0, 17);
    gate.branch(0xd0, entry + 13);
    gate.byte(0xa9, 0x80);
    gate.branch(0x30, entry + 19);
    gate.byte(0xe0, 18);
    gate.branch(0xd0, entry + 27);
    gate.byte(0xa9, 0x40);
    gate.word(0x8d, state);
    gate.byte(0xa9, 0);
    gate.byte(0x85, scratch);
    gate.op(0x60);
    gate.byte(0xa6, scratch);
    gate.branch(0x30, entry + 36);
    gate.byte(0xe0, 20);
    gate.branch(0x90, entry + 36);
    gate.op(0x60);
    for opcode in [0xca, 0x8a, 0x0a, 0xa8] {
        gate.op(opcode);
    }
    gate.word(0xb9, table);
    gate.byte(0x85, pointer);
    gate.word(0xb9, table + 1);
    gate.byte(0x85, pointer + 1);
    gate.byte(0xa0, 0);
    gate.byte(0xb1, pointer);
    gate.byte(0x85, header);
    assert_eq!(gate.pc(), entry + 56);
    gate.byte(0x24, header);
    gate.branch(0x10, entry + 63);
    gate.word(0x4c, triple);
    gate.branch(0x90, conditional);
    gate.word(0x4c, entry + 0xf0);
    gate.finish(&mut rom);
    put(&mut rom, entry + 0xf0, &[0x60]);
    let mut condition = Program::new(conditional);
    condition.word(0xae, bank);
    condition.word(0xbd, active);
    condition.branch(0xf0, join);
    condition.word(0xbd, active + 4);
    condition.byte(0x29, 0x20);
    condition.branch(0xd0, alternate);
    condition.word(0xbd, active);
    condition.word(0x20, helper);
    condition.branch(0xb0, join);
    condition.op(0x60);
    condition.finish(&mut rom);
    let mut single = Program::new(alternate);
    single.byte(0xe0, 0);
    single.branch(0xd0, alternate + 8);
    single.byte(0xa2, 0x24);
    single.branch(0xd0, alternate + 10);
    single.byte(0xa2, 0);
    single.word(0x8e, bank);
    assert_eq!(single.pc(), join);
    single.byte(0xa0, 0);
    single.byte(0xb1, pointer);
    single.word(0x9d, active + 4);
    single.byte(0xa5, scratch);
    single.word(0x9d, active);
    single.op(0x98);
    single.word(0x9d, active + 3);
    single.word(0x8c, bank + 1);
    single.word(0x9d, state + 0x40);
    single.word(0x9d, state + 0x41);
    single.op(0xc8);
    single.byte(0xb1, pointer);
    single.word(0x9d, active + 1);
    single.op(0xc8);
    single.byte(0xb1, pointer);
    single.word(0x9d, active + 2);
    single.op(0x60);
    single.finish(&mut rom);
    let mut three = Program::new(triple);
    three.branch(0xb0, triple + 0x30);
    three.byte(0xa0, 2);
    three.byte(0xa5, scratch);
    three.word(0x8d, state + 0x72);
    for (i, destination) in [0x80, 0x81, 0x96, 0x97, 0xac, 0xad].into_iter().enumerate() {
        if i != 0 {
            three.op(0xc8);
        }
        three.byte(0xb1, pointer);
        three.word(0x8d, state + destination);
    }
    three.op(0x60);
    three.finish(&mut rom);
    put(&mut rom, triple + 0x30, &[0x60]);
    let mut priority = Program::new(helper);
    priority.op(0x38);
    priority.byte(0xe9, 1);
    priority.byte(0x85, scratch + 8);
    for _ in 0..3 {
        priority.op(0x4a);
    }
    priority.byte(0x85, scratch + 9);
    priority.byte(0xa5, scratch + 8);
    priority.byte(0x29, 7);
    priority.op(0xa8);
    priority.byte(0xa5, scratch + 9);
    priority.op(0x18);
    priority.byte(0x69, 3);
    priority.byte(0x85, scratch + 9);
    priority.word(0xb9, mask);
    priority.byte(0xa4, scratch + 9);
    priority.byte(0x31, pointer);
    priority.op(0x18);
    priority.branch(0xf0, helper + 33);
    priority.op(0x38);
    priority.op(0x60);
    priority.finish(&mut rom);
    put(&mut rom, mask, &[0x80, 0x40, 0x20, 0x10, 8, 4, 2, 1]);
    for raw in 1_u16..20 {
        let record = 0x9400 + relocation + raw * 16;
        let target = if raw == 19 { 0x7000 } else { record };
        put(&mut rom, table + (raw - 1) * 2, &target.to_le_bytes());
        let stream = 0x9800 + relocation + raw * 32;
        if (9..=12).contains(&raw) {
            let mut prefix = vec![0xb4, 0xf1];
            for i in 0..3 {
                prefix.extend((stream + i * 4).to_le_bytes());
            }
            put(&mut rom, record, &prefix);
        } else {
            put(&mut rom, record, &[0x25, stream as u8, (stream >> 8) as u8]);
        }
    }
    for vector in [0xfffa, 0xfffc, 0xfffe] {
        put(&mut rom, vector, &0x8000_u16.to_le_bytes());
    }
    rom
}

struct Program {
    start: u16,
    bytes: Vec<u8>,
}

impl Program {
    fn new(start: u16) -> Self {
        Self {
            start,
            bytes: Vec::new(),
        }
    }
    fn pc(&self) -> u16 {
        self.start + self.bytes.len() as u16
    }
    fn op(&mut self, opcode: u8) {
        self.bytes.push(opcode);
    }
    fn byte(&mut self, opcode: u8, value: u8) {
        self.bytes.extend([opcode, value]);
    }
    fn word(&mut self, opcode: u8, value: u16) {
        self.op(opcode);
        self.bytes.extend(value.to_le_bytes());
    }
    fn branch(&mut self, opcode: u8, target: u16) {
        let relative = i8::try_from(i32::from(target) - i32::from(self.pc()) - 2).unwrap();
        self.byte(opcode, relative as u8);
    }
    fn finish(self, rom: &mut [u8]) {
        put(rom, self.start, &self.bytes);
    }
}

fn offset(rom: &[u8], address: u16) -> usize {
    16 + (usize::from(address) - 0x8000) % (usize::from(rom[4]) * 0x4000)
}

fn put(rom: &mut [u8], address: u16, bytes: &[u8]) {
    let at = offset(rom, address);
    rom[at..at + bytes.len()].copy_from_slice(bytes);
}

#[cfg(test)]
fn scan(rom: &[u8], work: u64, cancelled: bool) -> Result<Vec<super::DriverCandidate>, ScanStop> {
    let cancel = AtomicBool::new(cancelled);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut findings = Vec::new();
    super::scan(rom, &mut findings, &mut budget, 8)?;
    Ok(findings)
}

#[cfg(test)]
fn consumer(rom: &[u8]) -> super::super::CodeSelectorConsumer {
    scan(rom, 100_000, false).unwrap()[0]
        .code
        .as_ref()
        .unwrap()
        .selector_consumers[0]
        .clone()
}

#[test]
fn decoded_record_prefixes_keep_fixed_fields_and_source_evidence() {
    let rom = synthetic_record_rom();
    let found = consumer(&rom);
    assert_eq!(found.pointers.len(), 17);
    assert_eq!(found.records.len(), 16);
    assert_eq!(
        found
            .records
            .iter()
            .map(|record| record.streams.len())
            .sum::<usize>(),
        24
    );
    for record in &found.records {
        let triple = (9..=12).contains(&record.raw_selector);
        assert_eq!(record.header, if triple { 0xb4 } else { 0x25 });
        assert_eq!(
            record.prefix_span.offset,
            16 + 0x1400 + u32::from(record.raw_selector) * 16
        );
        assert_eq!(record.prefix_span.byte_len, if triple { 8 } else { 3 });
        for (i, stream) in record.streams.iter().enumerate() {
            assert_eq!(
                stream.entry_span.offset,
                record.prefix_span.offset + if triple { 2 + i as u32 * 2 } else { 1 }
            );
            assert_eq!(stream.entry_span.byte_len, 2);
            assert_eq!(
                stream.target_cpu_address,
                0x9800 + u16::from(record.raw_selector) * 32 + i as u16 * 4
            );
            assert_eq!(stream.target_span.unwrap().byte_len, 1);
            assert_eq!(stream.disposition, CodePointerDisposition::Unparsed);
        }
        assert!(
            record
                .evidence
                .iter()
                .any(|e| e.signature == "decoded-record-header-branch")
        );
        assert_eq!(
            record
                .evidence
                .iter()
                .any(|e| e.signature == "decoded-record-conditional-priority"),
            !triple
        );
        for evidence in &record.evidence {
            let start = evidence.span.offset as usize;
            assert_eq!(
                evidence.sha256,
                zeff_firmware::sha256_hex(&rom[start..start + evidence.span.byte_len as usize])
            );
        }
    }
}

#[test]
fn record_matching_normalizes_code_ram_and_mirrored_mapping() {
    let rom = record_rom(1, 0x4000, 0x20, 0x0300);
    let found = consumer(&rom);
    assert_eq!(found.entry_cpu_address, 0xc300);
    assert_eq!(found.pointer_address, 0x22);
    assert_eq!(found.records.len(), 16);
    assert_eq!(found.records[0].prefix_span.offset, 16 + 0x1410);
    assert_eq!(found.records[0].streams[0].target_cpu_address, 0xd820);
    assert_eq!(
        found.records[0].streams[0].target_span.unwrap().offset,
        16 + 0x1820
    );
}

#[test]
fn changed_paths_operands_and_helper_effects_hold_only_record_evidence() {
    for (address, value) in [
        (0x8338, 0xa5),
        (0x8339, 0x54),
        (0x833b, 0),
        (0x834b, 0x45),
        (0x8350, 0x10),
        (0x8355, 0x01),
        (0x838e, 1),
        (0x8390, 0x53),
        (0x8395, 0x51),
        (0x83b0, 0x13),
        (0x8400, 0x90),
        (0x8403, 1),
        (0x840a, 0x53),
        (0x840c, 0x52),
        (0x840d, 0),
        (0x8500, 0x18),
        (0x8504, 0x52),
        (0x8513, 4),
        (0x851c, 0x53),
        (0x851f, 0),
        (0x8540, 0),
    ] {
        let mut rom = synthetic_record_rom();
        put(&mut rom, address, &[value]);
        let found = consumer(&rom);
        assert_eq!(found.pointers.len(), 17, "selector survives {address:04x}");
        assert!(found.records.is_empty(), "record mutation at {address:04x}");
    }
}

#[test]
fn stream_heads_are_held_for_code_aperture_prefix_and_unmapped_targets() {
    let mut rom = synthetic_record_rom();
    for (raw, target) in [
        (1_u16, 0x8100_u16),
        (2, TABLE),
        (3, 0x9431),
        (4, 0x7000),
        (5, 0x9461),
    ] {
        put(&mut rom, 0x9401 + raw * 16, &target.to_le_bytes());
    }
    let found = consumer(&rom);
    assert_eq!(found.records.len(), 16);
    for (record, disposition) in found.records.iter().zip([
        CodePointerDisposition::DecodedCode,
        CodePointerDisposition::PointerTable,
        CodePointerDisposition::RecordPrefix,
        CodePointerDisposition::Unmapped,
    ]) {
        assert_eq!(record.streams[0].disposition, disposition);
        assert_eq!(
            record.streams[0].target_span.is_none(),
            disposition == CodePointerDisposition::Unmapped
        );
    }
    assert_eq!(
        found.records[4].streams[0].disposition,
        CodePointerDisposition::RecordPrefix
    );
}

#[test]
fn mapped_prefixes_reject_code_aperture_and_mirror_boundary_overlap() {
    for target in [0x80ff_u16, TABLE - 1, 0xbffe] {
        let mut rom = record_rom(1, 0, 0x50, 0x0600);
        put(&mut rom, TABLE, &target.to_le_bytes());
        let found = consumer(&rom);
        assert_eq!(
            found.pointers[0].disposition,
            CodePointerDisposition::Unparsed
        );
        assert_eq!(found.records.len(), 15, "prefix target {target:04x}");
        assert!(found.records.iter().all(|record| record.raw_selector != 1));
    }
}

#[test]
fn record_scans_charge_work_and_never_publish_partial_candidates() {
    let rom = synthetic_record_rom();
    assert_eq!(scan(&rom, 0, false), Err(ScanStop::WorkLimit));
    assert_eq!(scan(&rom, 100_000, true), Err(ScanStop::Cancelled));
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut findings = Vec::new();
    super::scan(&rom, &mut findings, &mut budget, 8).unwrap();
    let used = 100_000 - budget.remaining;
    for work in used - 32..used {
        let mut budget = Budget {
            cancel: &cancel,
            remaining: work,
        };
        let mut partial = Vec::new();
        assert_eq!(
            super::scan(&rom, &mut partial, &mut budget, 8),
            Err(ScanStop::WorkLimit)
        );
        assert!(partial.is_empty());
    }
}
