pub(crate) trait EnumLabel: Copy + PartialEq + 'static {
    fn label(self) -> &'static str;
    fn all_variants() -> &'static [Self];
}

pub(crate) fn enum_combo_box<E: EnumLabel>(ui: &mut egui::Ui, combo_label: &str, value: &mut E) {
    egui::ComboBox::from_label(combo_label)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for &variant in E::all_variants() {
                ui.selectable_value(value, variant, variant.label());
            }
        });
}
