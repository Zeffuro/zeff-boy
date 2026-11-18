use super::{DiagramAction, DiagramInteraction, DiagramKind, draw};
use crate::settings::BindingAction;

#[test]
fn continuous_dpad_keeps_four_actions_and_an_inert_center() {
    fn frame(context: &egui::Context, events: Vec<egui::Event>) -> DiagramInteraction {
        let mut interaction = None;
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_width(360.0);
                interaction = Some(draw(ui, DiagramKind::StandardGamepad, 0, None, None));
            },
        );
        interaction.unwrap()
    }
    for target in [
        None,
        Some(BindingAction::Up),
        Some(BindingAction::Down),
        Some(BindingAction::Left),
        Some(BindingAction::Right),
    ] {
        let context = egui::Context::default();
        let initial = frame(&context, vec![]);
        let center = |action| {
            initial
                .hit_rects
                .iter()
                .find(|(candidate, _)| *candidate == DiagramAction::Joypad(action))
                .unwrap()
                .1
                .center()
        };
        let position = target.map_or_else(
            || center(BindingAction::Up).lerp(center(BindingAction::Down), 0.5),
            center,
        );
        for pressed in [true, false] {
            let interaction = frame(
                &context,
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
            if !pressed {
                assert_eq!(interaction.activated, target.map(DiagramAction::Joypad));
            }
        }
    }
}

#[test]
fn full_size_diagrams_have_distinct_minimum_hit_targets() {
    for kind in DiagramKind::ALL {
        let context = egui::Context::default();
        let mut interaction = None;
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(360.0);
            interaction = Some(draw(ui, kind, 0, None, None));
        });
        let DiagramInteraction {
            hit_rects,
            diagram_rect,
            ..
        } = interaction.expect("diagram rendered");
        assert_eq!(hit_rects.len(), kind.actions().len(), "{kind:?}");
        for (action, rect) in &hit_rects {
            assert!(rect.width() >= 36.0, "{kind:?} {action:?}: {rect:?}");
            assert!(rect.height() >= 36.0, "{kind:?} {action:?}: {rect:?}");
            assert!(
                diagram_rect.expand(0.01).contains_rect(*rect),
                "{kind:?} {action:?} escapes {diagram_rect:?}: {rect:?}"
            );
        }
        for (index, (left_action, left)) in hit_rects.iter().enumerate() {
            for (right_action, right) in &hit_rects[index + 1..] {
                let overlap = left.intersect(*right);
                assert!(
                    overlap.width() <= 0.01 || overlap.height() <= 0.01,
                    "{kind:?} {left_action:?} overlaps {right_action:?}: {overlap:?}"
                );
            }
        }
    }
}
