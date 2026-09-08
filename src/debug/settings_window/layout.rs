use crate::debug::ui_helpers::EnumLabel;

use super::search::{self, SettingId};

const NARROW_WIDTH: f32 = 520.0;
const LABEL_WIDTH: f32 = 190.0;
const CONTROL_WIDTH: f32 = 240.0;
const ROW_HEIGHT: f32 = 26.0;

pub(super) fn row(
    ui: &mut egui::Ui,
    id: SettingId,
    label_override: Option<&str>,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let label = label_override.unwrap_or(search::metadata(id).title);
    let mut add = Some(add);
    let mut response = None;
    let mut label_id = None;
    if ui.available_width() < NARROW_WIDTH {
        ui.vertical(|ui| {
            label_id = Some(ui.add(egui::Label::new(label).wrap()).id);
            response = Some(add.take().unwrap()(ui));
        });
    } else {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(LABEL_WIDTH, ROW_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(LABEL_WIDTH);
                    ui.set_max_width(LABEL_WIDTH);
                    label_id = Some(ui.add(egui::Label::new(label).wrap()).id);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(CONTROL_WIDTH, ROW_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(CONTROL_WIDTH);
                    response = Some(add.take().unwrap()(ui));
                },
            );
        });
    }
    let response = response.unwrap().labelled_by(label_id.unwrap());
    search::target(ui, id, &response);
    response
}

pub(super) fn checkbox(
    ui: &mut egui::Ui,
    id: SettingId,
    label_override: Option<&str>,
    value: &mut bool,
) -> egui::Response {
    row(ui, id, label_override, |ui| ui.checkbox(value, ""))
}

pub(super) fn enum_combo<E: EnumLabel>(
    ui: &mut egui::Ui,
    id: SettingId,
    salt: &'static str,
    label_override: Option<&str>,
    value: &mut E,
) -> egui::Response {
    row(ui, id, label_override, |ui| {
        egui::ComboBox::from_id_salt(salt)
            .selected_text(value.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for &variant in E::all_variants() {
                    ui.selectable_value(value, variant, variant.label());
                }
            })
            .response
    })
}

pub(super) fn helper(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.scope(|ui| {
        ui.set_max_width(620.0_f32.min(ui.available_width()));
        ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
        ui.visuals_mut().override_text_color = Some(ui.visuals().weak_text_color());
        ui.add(egui::Label::new(text).wrap());
    });
}

pub(super) fn tab<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> egui::Response {
    let selected = *current == value;
    let mut response = ui.add(egui::Button::new(label).frame(false));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    if selected {
        ui.painter().line_segment(
            [response.rect.left_bottom(), response.rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            ui.visuals().widgets.active.corner_radius,
            ui.visuals().widgets.active.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    response
}
