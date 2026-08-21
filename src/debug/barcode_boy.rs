use super::DebugWindowState;

pub(crate) fn draw_scan_window(
    ctx: &egui::Context,
    state: &mut DebugWindowState,
    scan_allowed: bool,
) -> Option<String> {
    if !state.barcode_boy_scan_open {
        return None;
    }

    let mut open = true;
    let mut close = false;
    let mut submitted = None;
    egui::Window::new("Barcode Boy Scan")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.label("EAN-13");
            let response = ui.add_sized(
                [260.0, 24.0],
                egui::TextEdit::singleline(&mut state.barcode_boy_digits),
            );
            sanitize_ean_input(&mut state.barcode_boy_digits);

            let valid = valid_ean_input(&state.barcode_boy_digits);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(scan_allowed && valid, egui::Button::new("Scan"))
                    .clicked()
                    || (scan_allowed
                        && valid
                        && response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    submitted = Some(state.barcode_boy_digits.clone());
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                ui.label(format!("{}/13", state.barcode_boy_digits.len()));
            });
        });

    state.barcode_boy_scan_open = open && !close;
    if close && submitted.is_none() {
        state.barcode_boy_digits.clear();
    }
    submitted
}

fn sanitize_ean_input(digits: &mut String) {
    digits.retain(|character| character.is_ascii_digit());
    digits.truncate(13);
}

fn valid_ean_input(digits: &str) -> bool {
    digits.len() == 13 && digits.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ean_input_policy_filters_and_bounds_to_thirteen_ascii_digits() {
        let mut digits = "12A3456789012345".to_string();
        sanitize_ean_input(&mut digits);
        assert_eq!(digits, "1234567890123");
        assert!(valid_ean_input(&digits));
        assert!(!valid_ean_input("123456789012"));
        assert!(!valid_ean_input("123456789012X"));
    }
}
