#[path = "ordinary_clear_not_primary/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_primary/check.rs"]
mod check;
#[path = "ordinary_clear_not_primary/device.rs"]
mod device;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_primary/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_primary/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_primary/seed.rs"]
mod seed;

#[test]
fn primary_callbacks_and_reserve_faults() {
    let result = check::run(true);
    println!("ordinary primary count={} bad={}", result.0, result.1);
    assert_eq!(result, (3040, 0));
}

#[test]
fn source_literals_bind_whole_states() {
    assert_eq!(check::source_literals(true), (184, 0));
}

#[test]
fn seed_override_and_alias_seams() {
    assert_eq!(check::seams(true), (12, 0));
}
