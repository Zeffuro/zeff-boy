use crate::entry_axes;
use crate::literal::Value;

pub struct Hash(u64);

impl Hash {
    fn bytes(&mut self, value: &[u8]) {
        for byte in value {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(1099511628211);
        }
    }
    fn tag(&mut self, value: u8) {
        self.bytes(&[value]);
    }
    fn text(&mut self, value: &str) {
        self.tag(4);
        self.bytes(&(value.len() as u32).to_le_bytes());
        self.bytes(value.as_bytes());
    }
    pub fn value(&mut self, value: &Value) {
        match value {
            Value::Null => self.tag(0),
            Value::Bool(false) => self.tag(1),
            Value::Bool(true) => self.tag(2),
            Value::Integer(number) => {
                self.tag(3);
                self.bytes(&number.to_le_bytes());
            }
            Value::Text(text) => self.text(text),
            Value::Owned(text) => self.text(text),
            Value::Decimal(number) => self.text(&number.to_string()),
            Value::List(values) => {
                self.tag(5);
                self.bytes(&(values.len() as u32).to_le_bytes());
                for value in values {
                    self.value(value);
                }
            }
            Value::Object(fields) => {
                self.tag(6);
                self.bytes(&(fields.len() as u32).to_le_bytes());
                let mut fields = fields.iter().collect::<Vec<_>>();
                fields.sort_unstable_by_key(|(key, _)| *key);
                for (key, value) in fields {
                    self.text(key);
                    self.value(value);
                }
            }
        }
    }
}

pub fn all() -> (usize, usize) {
    let mut count = 0;
    let mut hash = Hash(14695981039346656037);
    entry_axes::each(|axis| {
        hash.value(&axis.value);
        count += 1;
    });
    (count, usize::from(hash.0 != 1085206134132874703))
}
