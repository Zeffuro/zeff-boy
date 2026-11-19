use super::*;

struct Reader {
    bytes: &'static [u8],
    at: usize,
}

impl Reader {
    fn number(&mut self) -> u64 {
        let bytes = self.bytes[self.at..self.at + 8].try_into().unwrap();
        self.at += 8;
        u64::from_le_bytes(bytes)
    }
    fn u32(&mut self) -> u32 {
        self.number().try_into().unwrap()
    }
    fn u16(&mut self) -> u16 {
        self.number().try_into().unwrap()
    }
    fn registers(&mut self) -> Registers {
        let mut r = Registers::new();
        r.d = std::array::from_fn(|_| self.u32());
        r.a = std::array::from_fn(|_| self.u32());
        r.usp = self.u32();
        r.ssp = self.u32();
        r.pc = self.u32();
        let sr = self.u16();
        r.set_status_register(sr);
        assert_eq!(r.status_register(), sr);
        r
    }
    fn memory(&mut self) -> BTreeMap<u32, u16> {
        let count = self.number();
        let mut words = BTreeMap::new();
        for _ in 0..count {
            assert!(words.insert(self.u32(), self.u16()).is_none());
        }
        words
    }
    fn event(&mut self) -> Event {
        let tag = self.number().try_into().unwrap();
        let clock = self.number();
        let logical_address = self.u32();
        let bus_address = self.u32();
        let fc = self.number();
        let kind = self.number();
        let value = self.u16();
        Event {
            tag,
            clock,
            access: BusAccess {
                logical_address,
                bus_address,
                function_code: function_code(fc),
                kind: match kind {
                    0 => AccessKind::Prefetch,
                    1 => AccessKind::Data,
                    2 => AccessKind::Stack,
                    3 => AccessKind::Vector,
                    _ => panic!("literal access kind"),
                },
                lanes: ByteLanes::Both,
            },
            value,
        }
    }
    fn row(&mut self) -> Row {
        let id = self.number();
        let opcode = self.u16();
        let count = self.number().try_into().unwrap();
        let final_fetch = self.number() == 1;
        let outcome = self.number().try_into().unwrap();
        let initial = self.registers();
        let initial_queue = [self.u16(), self.u16()];
        let initial_clock = self.number();
        let initial_memory = self.memory();
        let phase_registers = self.registers();
        let capture = Group0Capture {
            vector: self.number().try_into().unwrap(),
            internal_pc: self.u32(),
            phase_sr: self.u16(),
            ird: self.u16(),
            logical_address: self.u32(),
            function_code: function_code(self.number()),
            direction: Direction::Read,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        };
        let ssw = self.u16();
        let instruction_clock = self.number();
        let expected = self.registers();
        let queued = self.number() == 1;
        let words = [self.u16(), self.u16()];
        let clock = self.number();
        let entry_count = self.number().try_into().unwrap();
        let memory = self.memory();
        let event_count = self.number();
        let events = (0..event_count).map(|_| self.event()).collect();
        Row {
            id,
            opcode,
            count,
            final_fetch,
            outcome,
            initial,
            initial_queue,
            initial_clock,
            initial_memory,
            phase_registers,
            capture,
            ssw,
            instruction_clock,
            expected,
            queue: queued.then_some(words),
            clock,
            entry_count,
            memory,
            events,
        }
    }
}

fn function_code(value: u64) -> FunctionCode {
    match value {
        0 | 1 => FunctionCode::UserData,
        2 => FunctionCode::UserProgram,
        5 => FunctionCode::SupervisorData,
        6 => FunctionCode::SupervisorProgram,
        _ => panic!("literal FC"),
    }
}

pub(super) fn rows() -> Vec<Row> {
    let bytes = include_bytes!("first-literals.bin");
    assert_eq!(&bytes[..4], b"CIR1");
    let mut reader = Reader { bytes, at: 4 };
    let count = reader.number();
    assert_eq!(count, 64);
    let rows = (0..count).map(|_| reader.row()).collect();
    assert_eq!(reader.at, bytes.len());
    rows
}
