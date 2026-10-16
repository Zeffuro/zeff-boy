mod vdp_sat_live {
    #[path = "cases.rs"]
    mod cases;

    macro_rules! live_tests {
        ($($name:ident),+ $(,)?) => {
            $(#[test]
            fn $name() {
                cases::$name();
            })+
        };
    }

    live_tests!(
        cold_and_known_seeds,
        queued_partial_and_settled,
        all_entries_fields_and_aliases,
        aperture_boundaries_and_bit16,
        relocated_cache_and_live_fields,
        odd_invalidation_and_repair,
        pending_table_publication,
        segmented_horizon_and_copies,
        future_failure_atomicity,
    );
}

mod vdp_sprite_cache {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn unknown_and_rejections() {
        cases::unknown_and_rejections();
    }

    #[test]
    fn aperture_and_fields() {
        cases::aperture_and_fields();
    }

    #[test]
    fn stale_table_and_publication() {
        cases::stale_table_and_publication();
    }

    #[test]
    fn copies_and_raw_memory() {
        cases::copies_and_raw_memory();
    }
}

mod vdp_sprite_fetch {
    #[path = "cases.rs"]
    mod cases;
    #[path = "support.rs"]
    pub(crate) mod support;

    #[test]
    fn prepared_columns() {
        cases::prepared_columns();
    }

    #[test]
    fn selected_slots_and_live_fields() {
        cases::selected_slots_and_live_fields();
    }

    #[test]
    fn captured_metadata_and_live_patterns() {
        cases::captured_metadata_and_live_patterns();
    }

    #[test]
    fn wrap_positions_and_guards() {
        cases::wrap_positions_and_guards();
    }
}

mod vdp_sprite_line {
    #[path = "cases.rs"]
    mod cases;
    #[path = "composition.rs"]
    mod composition;
    #[path = "support.rs"]
    mod fixtures;
    pub(crate) use super::vdp_sprites::{composition as previous, support};

    #[test]
    fn geometry_and_clipping() {
        cases::geometry_and_clipping();
    }
    #[test]
    fn linked_alpha_priority() {
        cases::linked_alpha_priority();
    }
    #[test]
    fn budget_and_mask_guards() {
        cases::budget_and_mask_guards();
    }
    #[test]
    fn captured_pixels() {
        cases::captured_pixels();
    }
    #[test]
    fn background_merge() {
        composition::background_merge();
    }
    #[test]
    fn prepared_raster() {
        composition::prepared_raster();
    }
}

mod vdp_sprite_row {
    #[path = "cases.rs"]
    pub(crate) mod cases;

    #[test]
    fn vertical_bounds() {
        cases::vertical_bounds();
    }

    #[test]
    fn linked_capacity() {
        cases::linked_capacity();
    }

    #[test]
    fn unknown_and_malformed() {
        cases::unknown_and_malformed();
    }

    #[test]
    fn snapshot_and_fetch() {
        cases::snapshot_and_fetch();
    }
}

mod vdp_sprites {
    #[path = "cases.rs"]
    mod cases;
    #[path = "composition.rs"]
    pub(crate) mod composition;
    #[path = "support.rs"]
    pub(crate) mod support;

    #[test]
    fn fields_and_table_bounds() {
        cases::fields_and_table_bounds();
    }
    #[test]
    fn shapes_and_whole_flips() {
        cases::shapes_and_whole_flips();
    }
    #[test]
    fn clipping_and_pattern_wrap() {
        cases::clipping_and_pattern_wrap();
    }
    #[test]
    fn linked_priority_and_guards() {
        cases::linked_priority_and_guards();
    }
    #[test]
    fn row_limits() {
        cases::row_limits();
    }
    #[test]
    fn sprite_plane_priority() {
        composition::sprite_plane_priority();
    }
    #[test]
    fn shadow_highlight() {
        composition::shadow_highlight();
    }
    #[test]
    fn publication_and_fifo() {
        composition::publication_and_fifo();
    }
    #[test]
    fn sprite_raster() {
        composition::sprite_raster();
    }
}
