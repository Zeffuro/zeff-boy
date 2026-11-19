#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Value {
    Null,
    Bool(bool),
    Integer(u64),
    Text(&'static str),
    List(std::rc::Rc<[Value]>),
    Object(std::rc::Rc<[(&'static str, Value)]>),
}

impl Value {
    pub fn get(&self, key: &str) -> &Self {
        let Self::Object(fields) = self else {
            panic!("literal object")
        };
        let mut matches = fields.iter().filter(|(name, _)| *name == key);
        let result = &matches.next().expect("literal field").1;
        assert!(matches.next().is_none(), "duplicate literal field");
        result
    }
    pub fn single(&self) -> (&'static str, &Self) {
        let Self::Object(fields) = self else {
            panic!("literal variant")
        };
        assert_eq!(fields.len(), 1);
        (fields[0].0, &fields[0].1)
    }
    pub fn list(&self) -> &[Self] {
        let Self::List(values) = self else {
            panic!("literal list")
        };
        values
    }
    pub fn text(&self) -> &'static str {
        let Self::Text(text) = self else {
            panic!("literal string")
        };
        text
    }
    pub fn number(&self) -> u64 {
        match self {
            Self::Integer(value) => *value,
            Self::Text(text) => text.parse().expect("literal decimal"),
            _ => panic!("literal integer"),
        }
    }
    pub fn u32(&self) -> u32 {
        self.number().try_into().unwrap()
    }
    pub fn u16(&self) -> u16 {
        self.number().try_into().unwrap()
    }
    pub fn flag(&self) -> bool {
        let Self::Bool(flag) = self else {
            panic!("literal boolean")
        };
        *flag
    }
    pub fn optional(&self) -> Option<&Self> {
        (!matches!(self, Self::Null)).then_some(self)
    }
}

struct Reader {
    bytes: &'static [u8],
    at: usize,
    strings: Vec<&'static str>,
    numbers: Vec<u64>,
}

impl Reader {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let end = self.at.checked_add(N).unwrap();
        let result = self.bytes.get(self.at..end).unwrap().try_into().unwrap();
        self.at = end;
        result
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }
    fn string(&mut self) -> &'static str {
        let index = usize::from(self.u16());
        self.strings[index]
    }
    fn reference(&mut self, nodes: &[Value]) -> Value {
        let index = usize::try_from(self.u32()).unwrap();
        nodes.get(index).expect("prior literal node").clone()
    }
    fn value(&mut self, nodes: &[Value]) -> Value {
        match self.take::<1>()[0] {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => {
                let index = usize::from(self.u16());
                Value::Integer(self.numbers[index])
            }
            4 => Value::Text(self.string()),
            5 => {
                let count = self.u32();
                Value::List((0..count).map(|_| self.reference(nodes)).collect())
            }
            6 => {
                let count = self.u32();
                let mut keys = std::collections::BTreeSet::new();
                let fields = (0..count)
                    .map(|_| {
                        let key = self.string();
                        assert!(keys.insert(key), "duplicate literal key");
                        (key, self.reference(nodes))
                    })
                    .collect();
                Value::Object(fields)
            }
            _ => panic!("literal tag"),
        }
    }
}

pub(super) fn records() -> Vec<Value> {
    let mut reader = Reader {
        bytes: include_bytes!("first-literals.bin"),
        at: 0,
        strings: Vec::new(),
        numbers: Vec::new(),
    };
    assert_eq!(reader.take::<4>(), *b"CRF2");
    let count = reader.u32();
    assert_eq!(count, 3718);
    let strings = reader.u32();
    let numbers = reader.u32();
    let node_count = reader.u32();
    for _ in 0..strings {
        let length = usize::from(reader.u16());
        let end = reader.at.checked_add(length).unwrap();
        let text = std::str::from_utf8(reader.bytes.get(reader.at..end).unwrap()).unwrap();
        reader.at = end;
        reader.strings.push(text);
    }
    assert!(reader.strings.windows(2).all(|pair| pair[0] < pair[1]));
    for _ in 0..numbers {
        let number = u64::from_le_bytes(reader.take());
        reader.numbers.push(number);
    }
    assert!(reader.numbers.windows(2).all(|pair| pair[0] < pair[1]));
    let mut nodes = Vec::new();
    for _ in 0..node_count {
        let value = reader.value(&nodes);
        nodes.push(value);
    }
    let result = (0..count).map(|_| reader.reference(&nodes)).collect();
    assert_eq!(reader.at, reader.bytes.len());
    result
}
