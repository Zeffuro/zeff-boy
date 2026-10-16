#[path = "ordinary_clear_not_first/cases.rs"]
mod cases;
#[path = "ordinary_clear_not_first/check.rs"]
mod check;
#[path = "ordinary_clear_not_first/device.rs"]
mod device;
#[path = "ordinary_clear_not_first/literal.rs"]
mod literal;

#[test]
fn ordinary_clear_not_first() {
    assert_eq!(check::run(true), (1284, 0));
}

#[test]
fn retained_numeric_anchors() {
    assert_eq!(check::anchors(), (124, 0));
}
