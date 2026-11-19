#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_follow/device.rs"]
mod device;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_follow/follow_axes.rs"]
mod follow_axes;
#[path = "ordinary_clear_not_follow/follow_binding.rs"]
mod follow_binding;
#[path = "ordinary_clear_not_follow/follow_check.rs"]
mod follow_check;
#[path = "ordinary_clear_not_follow/follow_engine.rs"]
mod follow_engine;
#[path = "ordinary_clear_not_follow/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_entry/seed.rs"]
mod seed;

#[cfg(test)]
mod tests {
    #[test]
    fn follow6480() {
        assert_eq!(crate::follow_check::all(true), (6480, 0));
    }
    #[test]
    fn source_literals520() {
        assert_eq!(crate::follow_check::literals(true), (520, 0));
    }
    #[test]
    fn literal_probes520() {
        assert_eq!(crate::follow_check::probes(true), (520, 0));
    }
    #[test]
    fn source_axes520() {
        assert_eq!(crate::follow_check::source_axes(true), (520, 0));
    }
}
