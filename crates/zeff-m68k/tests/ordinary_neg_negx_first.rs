#[path = "ordinary_clear_not_first/cases.rs"]
pub mod cases;
#[path = "ordinary_neg_negx_first/check.rs"]
pub mod check;
#[path = "ordinary_clear_not_first/device.rs"]
pub mod device;
#[path = "ordinary_neg_negx_first/literal.rs"]
pub mod literal;

#[test]
fn negate_register_and_byte_fault_publication() {
    assert_eq!(check::remainder(true), (9808, 0, 14695981039346656037));
}
