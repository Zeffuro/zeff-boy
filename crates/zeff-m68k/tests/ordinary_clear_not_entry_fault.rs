#[path = "ordinary_clear_not_entry_fault/axis_binding.rs"]
mod axis_binding;
#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_entry/device.rs"]
mod device;
#[path = "ordinary_clear_not_entry_fault/entry_axes.rs"]
mod entry_axes;
#[path = "ordinary_clear_not_entry_fault/entry_binding.rs"]
mod entry_binding;
#[path = "ordinary_clear_not_entry_fault/entry_check.rs"]
mod entry_check;
#[path = "ordinary_clear_not_entry_fault/entry_engine.rs"]
mod entry_engine;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_entry_fault/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_entry_fault/seed.rs"]
mod seed;

#[cfg(test)]
mod tests {
    #[test]
    fn fault_bounded3642() {
        assert_eq!(crate::entry_check::bounded(true), (3642, 0));
    }
    #[test]
    fn fault_all380160() {
        assert_eq!(crate::entry_check::all(true), (380160, 0));
    }
    #[test]
    fn whole_literals3642() {
        assert_eq!(crate::entry_check::literals(true), (3642, 0));
    }
    #[test]
    fn report_literals2662() {
        assert_eq!(crate::entry_check::report_literals(true), (2662, 0));
    }
    #[test]
    fn reserve_literals980() {
        assert_eq!(crate::entry_check::reserve_literals(true), (980, 0));
    }
    #[test]
    fn all_axes380160() {
        assert_eq!(crate::axis_binding::all(), (380160, 0));
    }
}
