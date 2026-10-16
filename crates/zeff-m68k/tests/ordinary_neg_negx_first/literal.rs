include!("../ordinary_clear_not_first/literal.rs");
pub fn remainder_records() -> Vec<Value> {
    decode(include_bytes!("records.bin"), 9808)
}
