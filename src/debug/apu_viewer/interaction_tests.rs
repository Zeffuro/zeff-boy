use super::draw_apu_viewer_content;
use crate::debug::{ApuChannelDebug, ApuDebugInfo};

fn data(populated: bool) -> ApuDebugInfo {
    ApuDebugInfo {
        master_lines: vec!["Read-only current state".into()],
        master_waveform: Vec::new(),
        channels: if populated {
            ["Tone", "Noise"]
                .into_iter()
                .map(|name| ApuChannelDebug {
                    name,
                    enabled: true,
                    muted: true,
                    register_lines: Vec::new(),
                    detail_line: String::new(),
                    waveform: Vec::new(),
                })
                .collect()
        } else {
            Vec::new()
        },
        extra_sections: Vec::new(),
    }
}

fn render(
    context: &egui::Context,
    data: &ApuDebugInfo,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Vec<bool>>) {
    let mut action = None;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            action = draw_apu_viewer_content(ui, data);
        },
    );
    (output, action)
}

fn unmute_position(output: &egui::FullOutput) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == "Unmute All"
            {
                Some(text.pos + text.galley.size() * 0.5)
            } else {
                None
            }
        })
        .expect("Unmute All should be visible")
}

fn click(context: &egui::Context, data: &ApuDebugInfo, position: egui::Pos2) -> Option<Vec<bool>> {
    let mut action = None;
    for pressed in [true, false] {
        let (_, emitted) = render(
            context,
            data,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        if emitted.is_some() {
            assert!(action.is_none());
            action = emitted;
        }
    }
    action
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn empty_audio_inspection_unmute_cannot_emit_a_mute_action() {
    let context = egui::Context::default();
    context.global_style_mut(|style| style.animation_time = 0.0);
    let data = data(false);
    render(&context, &data, Vec::new());
    let (output, action) = render(&context, &data, Vec::new());
    assert!(action.is_none());
    let position = unmute_position(&output);
    assert_eq!(click(&context, &data, position), None);
    assert_eq!(render(&context, &data, Vec::new()).1, None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn populated_audio_viewer_unmute_still_emits_all_false() {
    let context = egui::Context::default();
    context.global_style_mut(|style| style.animation_time = 0.0);
    let data = data(true);
    render(&context, &data, Vec::new());
    let (output, action) = render(&context, &data, Vec::new());
    assert!(action.is_none());
    let position = unmute_position(&output);
    assert_eq!(click(&context, &data, position), Some(vec![false, false]));
}
