#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_entry/device.rs"]
#[allow(dead_code)]
mod device;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_reset/literal.rs"]
pub mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_reset/reset_axes.rs"]
mod reset_axes;
#[path = "ordinary_clear_not_reset/reset_binding.rs"]
mod reset_binding;
#[path = "ordinary_clear_not_reset/reset_check.rs"]
mod reset_check;
#[path = "ordinary_clear_not_reset/reset_device.rs"]
mod reset_device;
#[path = "ordinary_clear_not_reset/reset_engine.rs"]
mod reset_engine;
#[path = "ordinary_clear_not_reset/seed.rs"]
mod seed;

#[cfg(test)]
mod tests {
    #[test]
    fn all134128() {
        assert_eq!(crate::reset_check::all(true), (134128, 0));
    }
    #[test]
    fn bounded1428() {
        assert_eq!(crate::reset_check::bounded(true), (1428, 0));
    }
    #[test]
    fn whole_literals1428() {
        assert_eq!(crate::reset_check::literals(true), (1428, 0));
    }
}
