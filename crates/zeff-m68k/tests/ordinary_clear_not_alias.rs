#[path = "ordinary_clear_not_alias/axis_binding.rs"]
mod axis_binding;
#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_entry/device.rs"]
mod device;
#[path = "ordinary_clear_not_alias/entry_axes.rs"]
mod entry_axes;
#[path = "ordinary_clear_not_alias/entry_binding.rs"]
mod entry_binding;
#[path = "ordinary_clear_not_alias/entry_check.rs"]
mod entry_check;
#[path = "ordinary_clear_not_alias/entry_engine.rs"]
mod entry_engine;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_alias/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_alias/seed.rs"]
mod seed;

#[cfg(test)]
mod tests {
    #[test]
    fn alias_bounded575() {
        assert_eq!(crate::entry_check::bounded(true), (575, 0));
    }
    #[test]
    fn alias_all124416() {
        assert_eq!(crate::entry_check::all(true), (124416, 0));
    }
    #[test]
    fn whole_literals575() {
        assert_eq!(crate::entry_check::literals(true), (575, 0));
    }
    #[test]
    fn all_axes124416() {
        assert_eq!(crate::axis_binding::all(), (124416, 0));
    }
}
