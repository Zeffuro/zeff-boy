#[path = "ordinary_clear_not_preflight/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_preflight/device.rs"]
mod device;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_preflight/literal.rs"]
pub mod literal;
#[path = "ordinary_clear_not_preflight/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_preflight/preflight_axes.rs"]
pub mod preflight_axes;
#[path = "ordinary_clear_not_preflight/preflight_binding.rs"]
pub mod preflight_binding;
#[path = "ordinary_clear_not_preflight/preflight_check.rs"]
pub mod preflight_check;
#[path = "ordinary_clear_not_preflight/preflight_engine.rs"]
pub mod preflight_engine;
#[path = "ordinary_clear_not_preflight/preflight_setup.rs"]
mod preflight_setup;
#[path = "ordinary_clear_not_preflight/preflight_state.rs"]
mod preflight_state;
#[path = "ordinary_clear_not_preflight/seed.rs"]
mod seed;

pub use preflight_check::expected_rows;

#[cfg(test)]
mod tests {
    #[test]
    fn preflight17280() {
        assert_eq!(crate::preflight_check::all(true), (17280, 0));
    }
    #[test]
    fn source_literals491() {
        assert_eq!(crate::preflight_check::literals(true), (491, 0));
    }
    #[test]
    fn literal_probes491() {
        assert_eq!(crate::preflight_check::probes(true), (491, 0));
    }
    #[test]
    fn source_axes491() {
        assert_eq!(crate::preflight_check::source_axes(true), (491, 0));
    }
}
