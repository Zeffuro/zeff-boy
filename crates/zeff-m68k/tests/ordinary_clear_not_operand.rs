#[path = "ordinary_clear_not_operand/axis.rs"]
mod axis;
#[path = "ordinary_clear_not_operand/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_operand/check.rs"]
mod check;
#[path = "ordinary_clear_not_primary/device.rs"]
mod device;
#[path = "ordinary_clear_not_operand/directed.rs"]
mod directed;
#[path = "ordinary_clear_not_operand/extensions.rs"]
mod extensions;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_operand/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_operand/matrix.rs"]
mod matrix;
#[path = "ordinary_clear_not_primary/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_operand/seed.rs"]
mod seed;

#[test]
fn operand_whole_states() {
    assert_eq!(check::run(true), (8363048, 0));
}

#[test]
fn source_literals_bind_whole_states() {
    assert_eq!(check::source_literals(true), (1918, 0));
}

#[test]
fn source_literals_bind_all_axes() {
    assert_eq!(check::source_axes(true), (1918, 0));
}

#[test]
fn literal_cpu_probes() {
    assert_eq!(check::literal_probes(true), (1918, 0));
}
