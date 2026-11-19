#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_entry/device.rs"]
mod device;
#[path = "ordinary_clear_not_post_service/entry_axes.rs"]
mod entry_axes;
#[path = "ordinary_clear_not_alias/entry_binding.rs"]
mod entry_binding;
#[path = "ordinary_clear_not_alias/entry_engine.rs"]
mod entry_engine;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_post_service/literal.rs"]
pub mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_post_service/post.rs"]
pub mod post;
#[path = "ordinary_clear_not_entry/seed.rs"]
mod seed;

#[cfg(test)]
mod tests {
    #[test]
    fn source_all20736() {
        assert_eq!(crate::post::all(), (20736, 0));
    }
    #[test]
    fn source_whole949() {
        assert_eq!(crate::post::literals(true), (949, 0));
    }
}
