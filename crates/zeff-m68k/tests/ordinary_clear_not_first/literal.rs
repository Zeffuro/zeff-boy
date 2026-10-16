#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(u64),
    Text(&'static str),
    List(Vec<Value>),
    Object(Vec<(&'static str, Value)>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(key, value)| {
                        b.iter()
                            .find(|(other, _)| other == key)
                            .is_some_and(|(_, other)| value == other)
                    })
            }
            _ => false,
        }
    }
}

impl Value {
    pub fn get(&self, key: &str) -> &Self {
        self.find(key).unwrap_or_else(|| panic!("missing {key}"))
    }

    pub fn find(&self, key: &str) -> Option<&Self> {
        let Self::Object(fields) = self else {
            panic!("object required")
        };
        fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value)
    }

    pub fn list(&self) -> &[Self] {
        let Self::List(values) = self else {
            panic!("list required")
        };
        values
    }

    pub fn text(&self) -> &'static str {
        let Self::Text(value) = self else {
            panic!("text required")
        };
        value
    }

    pub fn number(&self) -> u64 {
        match self {
            Self::Integer(value) => *value,
            Self::Text(value) => value.parse().unwrap(),
            _ => panic!("number required"),
        }
    }

    pub fn boolean(&self) -> bool {
        let Self::Bool(value) = self else {
            panic!("boolean required")
        };
        *value
    }

    pub fn null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

struct Reader {
    bytes: &'static [u8],
    at: usize,
    strings: Vec<&'static str>,
}

impl Reader {
    fn take(&mut self, count: usize) -> &'static [u8] {
        let end = self.at.checked_add(count).unwrap();
        let slice = &self.bytes[self.at..end];
        self.at = end;
        slice
    }

    fn byte(&mut self) -> u8 {
        self.take(1)[0]
    }

    fn short(&mut self) -> usize {
        u16::from_le_bytes(self.take(2).try_into().unwrap()) as usize
    }

    fn count(&mut self) -> usize {
        u32::from_le_bytes(self.take(4).try_into().unwrap()) as usize
    }

    fn value(&mut self) -> Value {
        match self.byte() {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => Value::Integer(u64::from_le_bytes(self.take(8).try_into().unwrap())),
            4 => {
                let index = self.short();
                Value::Text(self.strings[index])
            }
            5 => {
                let count = self.count();
                Value::List((0..count).map(|_| self.value()).collect())
            }
            6 => {
                let count = self.count();
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    let index = self.short();
                    let key = self.strings[index];
                    assert!(!fields.iter().any(|(old, _)| *old == key));
                    fields.push((key, self.value()));
                }
                Value::Object(fields)
            }
            _ => panic!("literal tag"),
        }
    }
}

fn decode(bytes: &'static [u8], expected: usize) -> Vec<Value> {
    let mut reader = Reader {
        bytes,
        at: 0,
        strings: Vec::new(),
    };
    assert_eq!(reader.take(4), b"TRF1");
    assert_eq!(reader.count(), expected);
    let count = reader.count();
    for _ in 0..count {
        let size = reader.short();
        let text = std::str::from_utf8(reader.take(size)).unwrap();
        assert!(reader.strings.last().is_none_or(|old| *old < text));
        reader.strings.push(text);
    }
    let rows = (0..expected).map(|_| reader.value()).collect();
    assert_eq!(reader.at, bytes.len());
    rows
}

pub fn records() -> Vec<Value> {
    decode(include_bytes!("records.bin"), 1284)
}

pub fn anchors() -> Vec<Value> {
    decode(include_bytes!("anchors.bin"), 124)
}
