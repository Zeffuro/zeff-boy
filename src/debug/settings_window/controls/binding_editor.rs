use crate::input::{GamepadCommand, GamepadDeviceSnapshot, GamepadSnapshot, RuntimeGamepadId};
use crate::settings::{
    AxisBinding, AxisDirection, BindingExpression, BindingExpressionKind, BindingSet,
    BindingTarget, GameplayBindingSource, InputAxis, InputGameKey, InputScope,
    MAX_BINDING_ALTERNATIVES, MAX_CHORD_ATOMS, PhysicalBinding, ResolvedBindingSet, Settings,
};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureKind {
    Single,
    Chord,
}

const CAPTURE_ENGAGE_THRESHOLD: f32 = 0.55;
const CAPTURE_RELEASE_THRESHOLD: f32 = 0.4;

struct ActiveCapture {
    replace_id: Option<u64>,
    kind: CaptureKind,
    waiting_keys: Vec<KeyCode>,
    held_keys: Vec<KeyCode>,
    atoms: Vec<BindingExpression>,
    device: Option<RuntimeGamepadId>,
    neutralize_pending: bool,
    neutral_after_generation: Option<u64>,
    editor_neutral_ready: bool,
    chord_releasing: bool,
    started_at: crate::platform::Instant,
    overflowed: bool,
}

impl ActiveCapture {
    fn new(replace_id: Option<u64>, kind: CaptureKind, keyboard_down: &[KeyCode]) -> Self {
        Self {
            replace_id,
            kind,
            waiting_keys: unique_keys(keyboard_down),
            held_keys: Vec::new(),
            atoms: Vec::new(),
            device: None,
            neutralize_pending: true,
            neutral_after_generation: None,
            editor_neutral_ready: false,
            chord_releasing: false,
            started_at: crate::platform::Instant::now(),
            overflowed: false,
        }
    }
}

pub(crate) struct BindingEditor {
    scope: InputScope,
    target: BindingTarget,
    source: GameplayBindingSource,
    action_label: String,
    origin: InputScope,
    draft: BindingSet,
    explicit_unbound: bool,
    capture_chord: bool,
    capture: Option<ActiveCapture>,
    dirty: bool,
    close_requested: bool,
    stale_target: bool,
    consume_escape_release: bool,
    notice: Option<String>,
}

impl BindingEditor {
    pub(crate) fn open(
        scope: InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
        action_label: String,
        resolved: ResolvedBindingSet,
    ) -> Self {
        let explicit_unbound = resolved.value.is_none();
        Self {
            scope,
            target,
            source,
            action_label,
            origin: resolved.origin,
            draft: resolved.value.unwrap_or_default(),
            explicit_unbound,
            capture_chord: false,
            capture: None,
            dirty: false,
            close_requested: false,
            stale_target: false,
            consume_escape_release: false,
            notice: None,
        }
    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    pub(crate) fn target(&self) -> BindingTarget {
        self.target
    }

    pub(crate) fn keyboard_event(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> bool {
        if key == KeyCode::Escape && !pressed && self.consume_escape_release {
            self.consume_escape_release = false;
            return true;
        }
        if self.capture.is_none() {
            return false;
        }
        if key == KeyCode::Escape && pressed {
            let escape_was_preheld = self.source == GameplayBindingSource::Keyboard
                && self
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.waiting_keys.contains(&key));
            if !escape_was_preheld {
                self.capture = None;
                self.consume_escape_release = true;
                self.notice = Some("Capture cancelled.".into());
                return true;
            }
        }
        if self.source != GameplayBindingSource::Keyboard {
            return true;
        }
        let capture = self.capture.as_mut().expect("capture checked above");
        if capture.waiting_keys.contains(&key) {
            if !pressed {
                capture.waiting_keys.retain(|&held| held != key);
            }
            return true;
        }
        if !capture.waiting_keys.is_empty() || repeat {
            return true;
        }

        match capture.kind {
            CaptureKind::Single if pressed => {
                self.finish_capture(BindingExpression::keyboard(key));
            }
            CaptureKind::Single => {}
            CaptureKind::Chord if pressed => {
                if capture.chord_releasing {
                    return true;
                }
                if !capture.held_keys.contains(&key) {
                    capture.held_keys.push(key);
                }
                if !capture.atoms.iter().any(|atom| atom.references_key(key)) {
                    if capture.atoms.len() == MAX_CHORD_ATOMS {
                        capture.overflowed = true;
                        self.notice = Some(format!(
                            "A chord supports at most {MAX_CHORD_ATOMS} controls. Release the keys and try again."
                        ));
                    } else {
                        capture.atoms.push(BindingExpression::keyboard(key));
                    }
                }
            }
            CaptureKind::Chord => {
                if capture.held_keys.contains(&key) {
                    capture.chord_releasing = true;
                }
                capture.held_keys.retain(|&held| held != key);
                if capture.held_keys.is_empty() && !capture.atoms.is_empty() {
                    self.finish_chord_capture();
                }
            }
        }
        true
    }

    fn begin_capture(&mut self, replace_id: Option<u64>, keyboard_down: &[KeyCode]) {
        let kind = if self.capture_chord {
            CaptureKind::Chord
        } else {
            CaptureKind::Single
        };
        self.capture = Some(ActiveCapture::new(replace_id, kind, keyboard_down));
        self.notice = None;
    }

    fn finish_chord_capture(&mut self) {
        let Some(capture) = self.capture.as_ref() else {
            return;
        };
        if capture.overflowed {
            self.capture = None;
            self.notice = Some(format!(
                "A chord supports at most {MAX_CHORD_ATOMS} controls. Start capture to try again."
            ));
            return;
        }
        if capture.atoms.len() < 2 {
            self.capture = None;
            self.notice = Some("A chord needs at least two controls. Start capture to try again or turn off chord capture.".into());
            return;
        }
        let atoms =
            std::mem::take(&mut self.capture.as_mut().expect("capture checked above").atoms);
        self.finish_capture(BindingExpression::chord(atoms));
    }

    fn finish_capture(&mut self, expression: BindingExpression) {
        let replace_id = self.capture.as_ref().and_then(|capture| capture.replace_id);
        if let Err(error) = expression.validate(self.source) {
            self.notice = Some(error);
            self.capture = None;
            return;
        }
        let mut next = self.draft.clone();
        let result = if let Some(id) = replace_id {
            next.replace_expression(id, expression)
        } else if next.alternatives.is_empty() {
            next = BindingSet::new(expression);
            Ok(())
        } else {
            next.add_expression(expression).map(|_| ())
        };
        match result.and_then(|()| next.validate(self.source)) {
            Ok(()) => {
                self.draft = next;
                self.explicit_unbound = false;
                self.dirty = true;
                self.notice = Some("Binding captured.".into());
            }
            Err(error) => self.notice = Some(error),
        }
        self.capture = None;
    }

    fn poll_gamepad(&mut self, snapshot: &GamepadSnapshot) {
        if self.source != GameplayBindingSource::Gamepad {
            return;
        }
        if self
            .capture
            .as_ref()
            .and_then(|capture| capture.device)
            .is_some_and(|id| snapshot.devices.iter().all(|device| device.id != id))
        {
            self.capture = None;
            self.notice = Some("The controller disconnected during capture.".into());
            return;
        }
        let Some(capture) = self.capture.as_mut() else {
            return;
        };
        if capture.neutralize_pending || !snapshot.capture_active {
            return;
        }
        if !capture.editor_neutral_ready {
            let Some(generation) = capture.neutral_after_generation else {
                return;
            };
            if snapshot.sample_generation <= generation || !all_devices_neutral(snapshot) {
                return;
            }
            capture.editor_neutral_ready = true;
            return;
        }
        if !snapshot.capture_ready {
            return;
        }

        let device = if let Some(id) = capture.device {
            snapshot.devices.iter().find(|device| device.id == id)
        } else {
            snapshot
                .devices
                .iter()
                .find(|device| !device.waiting_for_neutral && !active_atoms(device).is_empty())
        };
        let Some(device) = device else {
            return;
        };
        capture.device.get_or_insert(device.id);
        let active = active_atoms(device);

        match capture.kind {
            CaptureKind::Single => {
                if let Some(candidate) = capture.atoms.first().cloned() {
                    if !atom_still_held(device, &candidate) {
                        self.finish_capture(candidate);
                    }
                } else if let Some(button) = device.buttons.first() {
                    self.finish_capture(BindingExpression::gamepad_button(button));
                } else if let Some(expression) = strongest_axis(device) {
                    capture.atoms.push(expression);
                }
            }
            CaptureKind::Chord => {
                if !capture.atoms.is_empty()
                    && capture
                        .atoms
                        .iter()
                        .any(|atom| !atom_still_held(device, atom))
                {
                    capture.chord_releasing = true;
                }
                if !capture.chord_releasing {
                    for expression in &active {
                        if !capture.atoms.contains(expression) {
                            if capture.atoms.len() == MAX_CHORD_ATOMS {
                                capture.overflowed = true;
                                self.notice = Some(format!(
                                    "A chord supports at most {MAX_CHORD_ATOMS} controls. Release the controller and try again."
                                ));
                                break;
                            }
                            capture.atoms.push(expression.clone());
                        }
                    }
                }
                if capture.chord_releasing
                    && !capture
                        .atoms
                        .iter()
                        .any(|atom| atom_still_held(device, atom))
                {
                    self.finish_chord_capture();
                }
            }
        }
    }

    fn persist_if_dirty(&mut self, settings: &mut Settings) {
        if !self.dirty || self.stale_target {
            return;
        }
        let value = (!self.explicit_unbound).then(|| self.draft.clone());
        match settings.set_binding_set(&self.scope, self.target, self.source, value) {
            Ok(()) => self.dirty = false,
            Err(error) => self.notice = Some(error),
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the modal coordinates independent settings, input, and game-lifecycle state"
)]
pub(crate) fn draw(
    ctx: &egui::Context,
    settings: &mut Settings,
    editor: &mut Option<BindingEditor>,
    snapshot: &GamepadSnapshot,
    commands: &mut Vec<GamepadCommand>,
    keyboard_down: &[KeyCode],
    active_game: Option<&InputGameKey>,
) {
    let Some(mut value) = editor.take() else {
        return;
    };
    value.stale_target = match &value.scope {
        InputScope::Game(game) => active_game != Some(game),
        _ => false,
    };
    if value.stale_target {
        value.capture = None;
        value.notice = Some(
            "The loaded game changed. Close this editor and reopen it for the current game.".into(),
        );
    }
    if value
        .capture
        .as_ref()
        .is_some_and(|capture| capture.started_at.elapsed().as_secs() >= 20)
    {
        value.capture = None;
        value.notice = Some("Capture timed out after 20 seconds.".into());
    }

    value.poll_gamepad(snapshot);
    value.persist_if_dirty(settings);
    let available = ctx.viewport_rect();
    let window_width = (available.width() - 16.0).clamp(80.0, 520.0);
    let content_height = (available.height() - 72.0).clamp(100.0, 620.0);
    let max_window_width = (available.width() - 8.0).max(80.0);
    let max_window_height = (available.height() - 8.0).max(100.0);
    let mut open = true;
    egui::Window::new(format!("Edit {} bindings", value.action_label))
        .id(egui::Id::new("typed_binding_editor"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(window_width)
        .max_width(max_window_width)
        .max_height(max_window_height)
        .constrain_to(available.shrink(4.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(content_height)
                .show(ui, |ui| {
                    draw_editor(ui, settings, &mut value, keyboard_down)
                });
        });

    if let Some(capture) = value.capture.as_mut()
        && value.source == GameplayBindingSource::Gamepad
        && capture.neutralize_pending
    {
        commands.push(GamepadCommand::Neutralize);
        capture.neutralize_pending = false;
        capture.neutral_after_generation = Some(snapshot.sample_generation);
    }
    value.persist_if_dirty(settings);
    if open && !value.close_requested {
        *editor = Some(value);
    }
}

fn draw_editor(
    ui: &mut egui::Ui,
    settings: &Settings,
    editor: &mut BindingEditor,
    keyboard_down: &[KeyCode],
) {
    ui.label(format!(
        "{} · {}",
        super::scope_label(&editor.scope),
        match editor.source {
            GameplayBindingSource::Keyboard => "Keyboard",
            GameplayBindingSource::Gamepad => "Controller",
        }
    ));
    if editor.origin != editor.scope {
        ui.label(
            egui::RichText::new(format!(
                "Currently inherited from {}. The first change creates an override here.",
                super::scope_label(&editor.origin)
            ))
            .weak(),
        );
    }
    if let Some(notice) = &editor.notice {
        ui.label(egui::RichText::new(notice).color(if editor.stale_target {
            ui.visuals().warn_fg_color
        } else {
            ui.visuals().text_color()
        }));
    }
    if editor.stale_target {
        if ui.button("Close").clicked() {
            editor.close_requested = true;
        }
        return;
    }

    ui.separator();
    if editor.explicit_unbound || editor.draft.alternatives.is_empty() {
        ui.label("Unbound in this scope.");
    } else {
        ui.strong("Alternatives");
    }

    let mut replace = None;
    let mut remove = None;
    let mut tuning_changed = false;
    for alternative in &mut editor.draft.alternatives {
        let id = alternative.id;
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(alternative.expression.label()).strong());
                if ui.small_button("Replace").clicked() {
                    replace = Some(id);
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(id);
                }
            });
            for warning in expression_warnings(
                settings,
                &editor.scope,
                editor.target,
                editor.source,
                &alternative.expression,
            ) {
                ui.label(
                    egui::RichText::new(format!("⚠ {warning}")).color(ui.visuals().warn_fg_color),
                );
            }
            tuning_changed |= draw_axis_options(
                ui,
                &mut alternative.expression,
                &format!("alternative_{id}"),
            );
        });
    }
    if tuning_changed {
        editor.dirty = true;
    }
    if let Some(id) = remove
        && editor.draft.remove_alternative(id)
    {
        editor.explicit_unbound = editor.draft.alternatives.is_empty();
        editor.dirty = true;
        editor.notice = Some("Binding removed.".into());
        editor.capture = None;
    }
    if let Some(id) = replace {
        editor.begin_capture(Some(id), keyboard_down);
    }

    ui.separator();
    ui.checkbox(
        &mut editor.capture_chord,
        format!("Capture a chord (2–{MAX_CHORD_ATOMS} controls)"),
    );
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                editor.draft.alternatives.len() < MAX_BINDING_ALTERNATIVES,
                egui::Button::new("Add alternative"),
            )
            .clicked()
        {
            editor.begin_capture(None, keyboard_down);
        }
        if editor.capture.is_some() && ui.button("Cancel capture").clicked() {
            editor.capture = None;
            editor.notice = Some("Capture cancelled.".into());
        }
        if ui.button("Done").clicked() {
            editor.capture = None;
            editor.close_requested = true;
        }
    });

    if let Some(capture) = &editor.capture {
        let text = if !capture.waiting_keys.is_empty() {
            "Release held keyboard keys before capture begins…"
        } else if editor.source == GameplayBindingSource::Gamepad {
            "Release held controller inputs, then press the new control…"
        } else if capture.kind == CaptureKind::Chord {
            "Hold 2–4 physical keys together, then release them…"
        } else {
            "Press a physical key…"
        };
        ui.label(egui::RichText::new(text).strong());
    }
    ui.label(
        egui::RichText::new(
            "Frontend shortcuts keep priority. The warnings above identify combinations that gameplay may not receive.",
        )
        .weak(),
    );
}

fn draw_axis_options(ui: &mut egui::Ui, expression: &mut BindingExpression, id_path: &str) -> bool {
    match &mut expression.kind {
        BindingExpressionKind::Axis(axis) => {
            let mut changed = false;
            egui::CollapsingHeader::new(format!("{} options", axis.axis.label()))
                .id_salt(format!("binding_axis_{id_path}"))
                .show(ui, |ui| {
                    changed |= ui
                        .add(egui::Slider::new(&mut axis.press_threshold, 0.1..=1.0).text("Engage"))
                        .changed();
                    if axis.release_threshold >= axis.press_threshold {
                        axis.release_threshold = (axis.press_threshold - 0.01).max(0.0);
                        changed = true;
                    }
                    changed |= ui
                        .add(
                            egui::Slider::new(
                                &mut axis.release_threshold,
                                0.0..=(axis.press_threshold - 0.01).max(0.0),
                            )
                            .text("Release"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut axis.transform.deadzone, 0.0..=0.95)
                                .text("Deadzone"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut axis.transform.curve, 0.1..=8.0)
                                .logarithmic(true)
                                .text("Curve"),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(&mut axis.transform.invert, "Invert axis")
                        .changed();
                });
            changed
        }
        BindingExpressionKind::Chord(atoms) => {
            let mut changed = false;
            for (index, atom) in atoms.iter_mut().enumerate() {
                changed |= draw_axis_options(ui, atom, &format!("{id_path}_{index}"));
            }
            changed
        }
        _ => false,
    }
}

fn active_atoms(device: &GamepadDeviceSnapshot) -> Vec<BindingExpression> {
    let mut atoms = device
        .buttons
        .iter()
        .map(BindingExpression::gamepad_button)
        .collect::<Vec<_>>();
    for axis in InputAxis::ALL {
        let value = axis.value(device.calibrated_left_stick, device.calibrated_right_stick);
        if value.abs() >= CAPTURE_ENGAGE_THRESHOLD {
            atoms.push(BindingExpression::axis(AxisBinding::new(
                axis,
                if value.is_sign_positive() {
                    AxisDirection::Positive
                } else {
                    AxisDirection::Negative
                },
            )));
        }
    }
    atoms
}

fn strongest_axis(device: &GamepadDeviceSnapshot) -> Option<BindingExpression> {
    InputAxis::ALL
        .into_iter()
        .map(|axis| {
            (
                axis,
                axis.value(device.calibrated_left_stick, device.calibrated_right_stick),
            )
        })
        .filter(|(_, value)| value.abs() >= CAPTURE_ENGAGE_THRESHOLD)
        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
        .map(|(axis, value)| {
            BindingExpression::axis(AxisBinding::new(
                axis,
                if value.is_sign_positive() {
                    AxisDirection::Positive
                } else {
                    AxisDirection::Negative
                },
            ))
        })
}

fn all_devices_neutral(snapshot: &GamepadSnapshot) -> bool {
    snapshot.devices.iter().all(|device| {
        device.buttons.is_empty()
            && InputAxis::ALL.into_iter().all(|axis| {
                axis.value(device.calibrated_left_stick, device.calibrated_right_stick)
                    .abs()
                    <= CAPTURE_RELEASE_THRESHOLD
            })
    })
}

fn atom_still_held(device: &GamepadDeviceSnapshot, expression: &BindingExpression) -> bool {
    match &expression.kind {
        BindingExpressionKind::GamepadButton(button) => device.buttons.contains(button),
        BindingExpressionKind::Axis(axis) => axis.evaluate(
            axis.axis
                .value(device.calibrated_left_stick, device.calibrated_right_stick),
            true,
        ),
        _ => false,
    }
}

fn expression_warnings(
    settings: &Settings,
    scope: &InputScope,
    target: BindingTarget,
    source: GameplayBindingSource,
    expression: &BindingExpression,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let conflicts = BindingTarget::all()
        .filter(|&other| other != target)
        .filter(|&other| {
            settings
                .binding_set(scope, other, source)
                .value
                .is_some_and(|set| {
                    set.alternatives
                        .iter()
                        .any(|other| other.expression == *expression)
                })
        })
        .map(target_label)
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        warnings.push(format!("Also assigned to {}.", conflicts.join(", ")));
    }

    let mut physical = Vec::new();
    collect_physical_bindings(expression, &mut physical);
    let mut frontend = Vec::new();
    for binding in &physical {
        frontend.extend(super::joypad::global_hotkey_conflicts(
            settings,
            Some(binding),
        ));
    }
    frontend.sort_unstable();
    frontend.dedup();
    if !frontend.is_empty() {
        warnings.push(format!(
            "Frontend mapping takes priority: {}.",
            frontend.join(", ")
        ));
    }

    let keys = expression.keyboard_keys();
    if keys.contains(&KeyCode::ShiftLeft) {
        warnings.push("Left Shift is reserved for Turbo before gameplay input.".into());
    }
    if keys.contains(&KeyCode::KeyR)
        && keys
            .iter()
            .any(|key| matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight))
    {
        warnings.push("Ctrl+R is reserved for reset before gameplay input.".into());
    }
    if keys.contains(&KeyCode::Enter)
        && keys
            .iter()
            .any(|key| matches!(key, KeyCode::AltLeft | KeyCode::AltRight))
    {
        warnings.push("Alt+Enter is reserved for fullscreen before gameplay input.".into());
    }
    if keys.iter().any(|key| {
        matches!(
            key,
            KeyCode::Digit0
                | KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
        )
    }) {
        warnings.push("Number-row keys select the global save-state slot before gameplay mappings, and also control the ColecoVision keypad while that system is active.".into());
    }
    if keys
        .iter()
        .any(|key| matches!(key, KeyCode::Minus | KeyCode::Equal))
    {
        warnings.push(
            "Minus and Equal control the ColecoVision keypad before gameplay mappings while that system is active."
                .into(),
        );
    }
    warnings
}

fn collect_physical_bindings(expression: &BindingExpression, output: &mut Vec<PhysicalBinding>) {
    match &expression.kind {
        BindingExpressionKind::Keyboard(key) => output.push(PhysicalBinding::Keyboard(*key)),
        BindingExpressionKind::GamepadButton(button) => {
            output.push(PhysicalBinding::Gamepad(button.clone()));
        }
        BindingExpressionKind::Chord(atoms) => {
            for atom in atoms {
                collect_physical_bindings(atom, output);
            }
        }
        BindingExpressionKind::Axis(_) | BindingExpressionKind::Unknown(_) => {}
    }
}

fn target_label(target: BindingTarget) -> String {
    match target {
        BindingTarget::Joypad { player, action } => format!("Player {player} {action:?}"),
        BindingTarget::WonderSwan(action) => format!("WonderSwan {}", action.label()),
        BindingTarget::Tilt(action) => format!("Tilt {action:?}"),
    }
}

fn unique_keys(keys: &[KeyCode]) -> Vec<KeyCode> {
    let mut result = Vec::new();
    for &key in keys {
        if !result.contains(&key) {
            result.push(key);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::GamepadDeviceSnapshot;
    use crate::settings::{BindingAction, GamepadFingerprint, InputScope};

    fn editor(source: GameplayBindingSource) -> BindingEditor {
        BindingEditor::open(
            InputScope::Global,
            BindingTarget::Joypad {
                player: 1,
                action: BindingAction::A,
            },
            source,
            "A".into(),
            Settings::default().binding_set(
                &InputScope::Global,
                BindingTarget::Joypad {
                    player: 1,
                    action: BindingAction::A,
                },
                source,
            ),
        )
    }

    fn snapshot(
        generation: u64,
        buttons: &[&str],
        left: (f32, f32),
        right: (f32, f32),
    ) -> GamepadSnapshot {
        GamepadSnapshot {
            devices: vec![GamepadDeviceSnapshot {
                id: RuntimeGamepadId(7),
                fingerprint: GamepadFingerprint {
                    name: "Test pad".into(),
                    uuid: "test-pad".into(),
                },
                buttons: buttons.iter().map(|button| (*button).to_owned()).collect(),
                left_stick: left,
                right_stick: right,
                calibrated_left_stick: left,
                calibrated_right_stick: right,
                waiting_for_neutral: false,
            }],
            capture_active: true,
            capture_ready: true,
            sample_generation: generation,
            ..Default::default()
        }
    }

    fn arm_gamepad(editor: &mut BindingEditor, chord: bool) {
        editor.capture_chord = chord;
        editor.begin_capture(None, &[]);
        let capture = editor.capture.as_mut().unwrap();
        capture.neutralize_pending = false;
        capture.neutral_after_generation = Some(1);
    }

    #[test]
    fn keyboard_capture_waits_for_preheld_keys() {
        let mut editor = editor(GameplayBindingSource::Keyboard);
        editor.begin_capture(None, &[KeyCode::KeyQ]);
        assert!(editor.keyboard_event(KeyCode::KeyW, true, false));
        assert!(editor.capture.is_some());
        assert!(editor.keyboard_event(KeyCode::KeyQ, false, false));
        assert!(editor.keyboard_event(KeyCode::KeyW, true, false));
        assert!(editor.capture.is_none());
        assert_eq!(
            editor.draft.alternatives.last().unwrap().expression,
            BindingExpression::keyboard(KeyCode::KeyW)
        );
    }

    #[test]
    fn keyboard_chord_finishes_after_every_key_is_released() {
        let mut editor = editor(GameplayBindingSource::Keyboard);
        editor.capture_chord = true;
        editor.begin_capture(None, &[]);
        editor.keyboard_event(KeyCode::ControlLeft, true, false);
        editor.keyboard_event(KeyCode::KeyZ, true, false);
        editor.keyboard_event(KeyCode::ControlLeft, false, false);
        assert!(editor.capture.is_some());
        editor.keyboard_event(KeyCode::KeyZ, false, false);
        assert!(editor.capture.is_none());
        assert!(matches!(
            &editor.draft.alternatives.last().unwrap().expression.kind,
            BindingExpressionKind::Chord(atoms) if atoms.len() == 2
        ));
    }

    #[test]
    fn gamepad_axis_waits_for_fresh_neutral_and_release() {
        let mut editor = editor(GameplayBindingSource::Gamepad);
        arm_gamepad(&mut editor, false);

        editor.poll_gamepad(&snapshot(2, &[], (0.0, 0.0), (0.8, 0.0)));
        assert!(!editor.capture.as_ref().unwrap().editor_neutral_ready);
        editor.poll_gamepad(&snapshot(3, &[], (0.0, 0.0), (0.0, 0.0)));
        assert!(editor.capture.as_ref().unwrap().editor_neutral_ready);

        editor.poll_gamepad(&snapshot(4, &[], (0.0, 0.0), (0.8, 0.0)));
        assert!(editor.capture.is_some());
        editor.poll_gamepad(&snapshot(5, &[], (0.0, 0.0), (0.45, 0.0)));
        assert!(editor.capture.is_some());
        editor.poll_gamepad(&snapshot(6, &[], (0.0, 0.0), (0.39, 0.0)));
        assert!(editor.capture.is_none());
        assert!(matches!(
            &editor.draft.alternatives.last().unwrap().expression.kind,
            BindingExpressionKind::Axis(axis)
                if axis.axis == InputAxis::RightX && axis.direction == AxisDirection::Positive
        ));
    }

    #[test]
    fn controller_chord_freezes_when_release_begins() {
        let mut editor = editor(GameplayBindingSource::Gamepad);
        arm_gamepad(&mut editor, true);
        editor.poll_gamepad(&snapshot(2, &[], (0.0, 0.0), (0.0, 0.0)));
        editor.poll_gamepad(&snapshot(3, &["South"], (0.0, 0.0), (0.0, 0.0)));
        editor.poll_gamepad(&snapshot(4, &["South", "East"], (0.0, 0.0), (0.0, 0.0)));
        editor.poll_gamepad(&snapshot(5, &["East", "North"], (0.0, 0.0), (0.0, 0.0)));
        editor.poll_gamepad(&snapshot(6, &["North"], (0.0, 0.0), (0.0, 0.0)));

        assert!(editor.capture.is_none());
        let BindingExpressionKind::Chord(atoms) =
            &editor.draft.alternatives.last().unwrap().expression.kind
        else {
            panic!("expected captured chord");
        };
        assert_eq!(
            atoms,
            &vec![
                BindingExpression::gamepad_button("South"),
                BindingExpression::gamepad_button("East"),
            ]
        );
    }

    #[test]
    fn escape_cancels_controller_capture_and_consumes_release() {
        let mut editor = editor(GameplayBindingSource::Gamepad);
        arm_gamepad(&mut editor, false);
        assert!(editor.keyboard_event(KeyCode::Escape, true, false));
        assert!(editor.capture.is_none());
        assert!(editor.keyboard_event(KeyCode::Escape, false, false));
        assert!(!editor.keyboard_event(KeyCode::Escape, false, false));
    }
}
