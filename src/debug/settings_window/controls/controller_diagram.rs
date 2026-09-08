use crate::emu_backend::ActiveSystem;
use crate::input::HostButton;
use crate::settings::{BindingAction, WonderSwanButton};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum DiagramKind {
    StandardGamepad,
    GameBoy,
    GameBoyAdvance,
    Nes,
    PceTwoButton,
    PceSixButton,
    SegaMasterSystem,
    GameGear,
    Sg1000,
    Coleco,
    WonderSwan,
}

impl DiagramKind {
    pub(crate) const ALL: [Self; 11] = [
        Self::StandardGamepad,
        Self::GameBoy,
        Self::GameBoyAdvance,
        Self::Nes,
        Self::PceTwoButton,
        Self::PceSixButton,
        Self::SegaMasterSystem,
        Self::GameGear,
        Self::Sg1000,
        Self::Coleco,
        Self::WonderSwan,
    ];

    pub(crate) const fn for_system(system: ActiveSystem) -> Self {
        match system {
            ActiveSystem::GameBoy => Self::GameBoy,
            ActiveSystem::GameBoyAdvance => Self::GameBoyAdvance,
            ActiveSystem::Nes => Self::Nes,
            ActiveSystem::Pce => Self::PceTwoButton,
            ActiveSystem::MasterSystem => Self::SegaMasterSystem,
            ActiveSystem::GameGear => Self::GameGear,
            ActiveSystem::Sg1000 => Self::Sg1000,
            ActiveSystem::Coleco => Self::Coleco,
            ActiveSystem::WonderSwan => Self::WonderSwan,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::StandardGamepad => "Standard gamepad",
            Self::GameBoy => "GB / GBC",
            Self::GameBoyAdvance => "Game Boy Advance",
            Self::Nes => "NES",
            Self::PceTwoButton => "PC Engine 2-button",
            Self::PceSixButton => "PC Engine 6-button",
            Self::SegaMasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Sg1000 => "SG-1000",
            Self::Coleco => "ColecoVision",
            Self::WonderSwan => "WonderSwan",
        }
    }

    pub(crate) fn actions(self) -> Vec<DiagramAction> {
        use BindingAction::*;
        let digital = match self {
            Self::StandardGamepad => vec![Up, Down, Left, Right, A, B, X, Y, L, R, Start, Select],
            Self::GameBoy | Self::Nes => vec![Up, Down, Left, Right, A, B, Start, Select],
            Self::GameBoyAdvance => vec![Up, Down, Left, Right, A, B, L, R, Start, Select],
            Self::PceTwoButton => vec![Up, Down, Left, Right, A, B, Start, Select],
            Self::PceSixButton => vec![Up, Down, Left, Right, A, B, L, R, X, Y, Start, Select],
            Self::SegaMasterSystem | Self::Sg1000 => vec![Up, Down, Left, Right, A, B],
            Self::GameGear => vec![Up, Down, Left, Right, A, B, Start],
            Self::Coleco => vec![Up, Down, Left, Right, A, B, Start, Select],
            Self::WonderSwan => {
                return WonderSwanButton::ALL
                    .iter()
                    .copied()
                    .map(DiagramAction::WonderSwan)
                    .collect();
            }
        };
        digital.into_iter().map(DiagramAction::Joypad).collect()
    }

    pub(crate) fn action_label(self, action: DiagramAction) -> &'static str {
        use BindingAction::*;
        match (self, action) {
            (_, DiagramAction::WonderSwan(button)) => button.label(),
            (Self::PceTwoButton | Self::PceSixButton, DiagramAction::Joypad(A)) => "I",
            (Self::PceTwoButton | Self::PceSixButton, DiagramAction::Joypad(B)) => "II",
            (Self::PceSixButton, DiagramAction::Joypad(L)) => "III",
            (Self::PceSixButton, DiagramAction::Joypad(R)) => "IV",
            (Self::PceSixButton, DiagramAction::Joypad(X)) => "V",
            (Self::PceSixButton, DiagramAction::Joypad(Y)) => "VI",
            (Self::PceTwoButton | Self::PceSixButton, DiagramAction::Joypad(Start)) => "Run",
            (Self::PceTwoButton | Self::PceSixButton, DiagramAction::Joypad(Select)) => "Select",
            (Self::SegaMasterSystem | Self::Sg1000, DiagramAction::Joypad(A)) => "1",
            (Self::SegaMasterSystem | Self::Sg1000, DiagramAction::Joypad(B)) => "2",
            (Self::GameGear, DiagramAction::Joypad(A)) => "1",
            (Self::GameGear, DiagramAction::Joypad(B)) => "2",
            (Self::Coleco, DiagramAction::Joypad(A)) => "Left side",
            (Self::Coleco, DiagramAction::Joypad(B)) => "Right side",
            (Self::Coleco, DiagramAction::Joypad(Select)) => "Keypad 1",
            (Self::Coleco, DiagramAction::Joypad(Start)) => "Keypad 2",
            (_, DiagramAction::Joypad(Up)) => "Up",
            (_, DiagramAction::Joypad(Down)) => "Down",
            (_, DiagramAction::Joypad(Left)) => "Left",
            (_, DiagramAction::Joypad(Right)) => "Right",
            (_, DiagramAction::Joypad(A)) => "A",
            (_, DiagramAction::Joypad(B)) => "B",
            (_, DiagramAction::Joypad(X)) => "X",
            (_, DiagramAction::Joypad(Y)) => "Y",
            (_, DiagramAction::Joypad(L)) => "L",
            (_, DiagramAction::Joypad(R)) => "R",
            (_, DiagramAction::Joypad(Start)) => "Start",
            (_, DiagramAction::Joypad(Select)) => "Select",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagramAction {
    Joypad(BindingAction),
    WonderSwan(WonderSwanButton),
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    kind: DiagramKind,
    pressed_host_mask: u16,
    selected: Option<DiagramAction>,
) -> Option<DiagramAction> {
    let width = ui.available_width().min(420.0);
    let height = width
        * if kind == DiagramKind::Coleco {
            0.94
        } else {
            0.60
        };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let palette = Palette::from_ui(ui);
    let mut diagram = DiagramPainter {
        ui,
        painter,
        rect,
        kind,
        pressed_host_mask,
        selected,
        palette,
        clicked: None,
    };

    match kind {
        DiagramKind::GameBoy => diagram.draw_game_boy(),
        DiagramKind::GameBoyAdvance | DiagramKind::GameGear => diagram.draw_handheld(),
        DiagramKind::Nes | DiagramKind::SegaMasterSystem | DiagramKind::Sg1000 => {
            diagram.draw_nes()
        }
        DiagramKind::PceTwoButton => diagram.draw_pce_two(),
        DiagramKind::PceSixButton => diagram.draw_pce_six(),
        DiagramKind::WonderSwan => diagram.draw_wonderswan(),
        DiagramKind::Coleco => diagram.draw_coleco(),
        _ => diagram.draw_pad(),
    }
    diagram.clicked
}

#[derive(Clone, Copy)]
struct Palette {
    body: egui::Color32,
    control: egui::Color32,
    accent: egui::Color32,
    selected: egui::Color32,
    text: egui::Color32,
    outline: egui::Color32,
}

impl Palette {
    fn from_ui(ui: &egui::Ui) -> Self {
        let visuals = ui.visuals();
        Self {
            body: visuals.widgets.inactive.bg_fill,
            control: visuals.extreme_bg_color,
            accent: visuals.selection.bg_fill,
            selected: egui::Color32::from_rgb(244, 178, 76),
            text: visuals.text_color(),
            outline: visuals.widgets.noninteractive.bg_stroke.color,
        }
    }
}

struct DiagramPainter<'a> {
    ui: &'a mut egui::Ui,
    painter: egui::Painter,
    rect: egui::Rect,
    kind: DiagramKind,
    pressed_host_mask: u16,
    selected: Option<DiagramAction>,
    palette: Palette,
    clicked: Option<DiagramAction>,
}

impl DiagramPainter<'_> {
    fn pos(&self, x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(
            self.rect.left() + self.rect.width() * x / 360.0,
            self.rect.top() + self.rect.width() * y / 360.0,
        )
    }

    fn scale(&self, value: f32) -> f32 {
        value * self.rect.width() / 360.0
    }

    fn title(&self, text: &str) {
        self.painter.text(
            self.pos(180.0, 15.0),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(14.0),
            self.palette.text,
        );
    }

    fn shell(&self, rect: egui::Rect, radius: f32) {
        self.painter.rect(
            rect,
            radius,
            self.palette.body,
            egui::Stroke::new(1.0, self.palette.outline),
            egui::StrokeKind::Inside,
        );
        let highlight = egui::Rect::from_min_max(
            rect.min + egui::vec2(radius * 0.7, radius * 0.45),
            egui::pos2(rect.max.x - radius * 0.7, rect.min.y + radius * 0.45),
        );
        self.painter.line_segment(
            [highlight.min, highlight.max],
            egui::Stroke::new(1.0, self.palette.outline),
        );
    }

    fn screen(&self, rect: egui::Rect) {
        self.painter.rect(
            rect,
            self.scale(6.0),
            self.palette.control,
            egui::Stroke::new(1.0, self.palette.outline),
            egui::StrokeKind::Inside,
        );
        self.painter.rect_filled(
            rect.shrink(self.scale(5.0)),
            self.scale(3.0),
            egui::Color32::from_rgb(12, 15, 18),
        );
    }

    fn draw_pad(&mut self) {
        self.title(self.kind.label());
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 112.0),
            egui::vec2(self.scale(326.0), self.scale(145.0)),
        );
        if self.kind == DiagramKind::StandardGamepad {
            for center in [self.pos(73.0, 124.0), self.pos(287.0, 124.0)] {
                self.painter.circle(
                    center,
                    self.scale(55.0),
                    self.palette.body,
                    egui::Stroke::new(1.0, self.palette.outline),
                );
            }
            self.shell(
                egui::Rect::from_center_size(
                    self.pos(180.0, 106.0),
                    egui::vec2(self.scale(250.0), self.scale(116.0)),
                ),
                self.scale(34.0),
            );
        } else {
            self.shell(body, self.scale(38.0));
        }

        let actions = self.kind.actions();
        self.draw_dpad(&actions);
        self.draw_face_buttons(&actions);
        self.draw_center_buttons(&actions);
        if self.kind != DiagramKind::PceSixButton
            && actions.contains(&DiagramAction::Joypad(BindingAction::L))
        {
            self.control_rect(
                egui::Rect::from_center_size(
                    self.pos(82.0, 51.0),
                    egui::vec2(self.scale(66.0), self.scale(17.0)),
                ),
                DiagramAction::Joypad(BindingAction::L),
            );
        }
        if self.kind != DiagramKind::PceSixButton
            && actions.contains(&DiagramAction::Joypad(BindingAction::R))
        {
            self.control_rect(
                egui::Rect::from_center_size(
                    self.pos(278.0, 51.0),
                    egui::vec2(self.scale(66.0), self.scale(17.0)),
                ),
                DiagramAction::Joypad(BindingAction::R),
            );
        }
    }

    fn draw_game_boy(&mut self) {
        self.title(self.kind.label());
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 117.0),
            egui::vec2(self.scale(154.0), self.scale(188.0)),
        );
        self.shell(body, self.scale(16.0));
        let screen = egui::Rect::from_center_size(
            self.pos(180.0, 79.0),
            egui::vec2(self.scale(104.0), self.scale(57.0)),
        );
        self.screen(screen);
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 145.0, 146.0);
        self.draw_face_at(
            &actions,
            &[
                (BindingAction::B, 211.0, 146.0),
                (BindingAction::A, 239.0, 129.0),
            ],
        );
        self.draw_center_at(&actions, 194.0, 149.0, 211.0);
    }

    fn draw_handheld(&mut self) {
        self.title(self.kind.label());
        if self.kind == DiagramKind::GameBoyAdvance {
            for center in [self.pos(74.0, 111.0), self.pos(286.0, 111.0)] {
                self.painter.circle(
                    center,
                    self.scale(62.0),
                    self.palette.body,
                    egui::Stroke::new(1.0, self.palette.outline),
                );
            }
            self.shell(
                egui::Rect::from_center_size(
                    self.pos(180.0, 105.0),
                    egui::vec2(self.scale(262.0), self.scale(126.0)),
                ),
                self.scale(48.0),
            );
        } else {
            let body = egui::Rect::from_center_size(
                self.pos(180.0, 112.0),
                egui::vec2(self.scale(326.0), self.scale(144.0)),
            );
            self.shell(body, self.scale(31.0));
        }
        let screen = egui::Rect::from_center_size(
            self.pos(180.0, 103.0),
            egui::vec2(self.scale(116.0), self.scale(62.0)),
        );
        self.screen(screen);
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 74.0, 118.0);
        self.draw_face_at(
            &actions,
            &[
                (BindingAction::B, 271.0, 130.0),
                (BindingAction::A, 301.0, 112.0),
            ],
        );
        self.draw_center_at(&actions, 156.0, 161.0, 204.0);
        if self.kind == DiagramKind::GameBoyAdvance {
            self.control_rect(
                egui::Rect::from_center_size(
                    self.pos(76.0, 52.0),
                    egui::vec2(self.scale(70.0), self.scale(17.0)),
                ),
                DiagramAction::Joypad(BindingAction::L),
            );
            self.control_rect(
                egui::Rect::from_center_size(
                    self.pos(284.0, 52.0),
                    egui::vec2(self.scale(70.0), self.scale(17.0)),
                ),
                DiagramAction::Joypad(BindingAction::R),
            );
        }
    }

    fn draw_nes(&mut self) {
        self.title(self.kind.label());
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 112.0),
            egui::vec2(self.scale(320.0), self.scale(126.0)),
        );
        self.shell(body, self.scale(7.0));
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 82.0, 112.0);
        self.draw_face_at(
            &actions,
            &[
                (BindingAction::B, 264.0, 128.0),
                (BindingAction::A, 299.0, 111.0),
            ],
        );
        self.draw_center_at(&actions, 153.0, 161.0, 207.0);
    }

    fn draw_pce_six(&mut self) {
        self.title(self.kind.label());
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 112.0),
            egui::vec2(self.scale(326.0), self.scale(145.0)),
        );
        self.shell(body, self.scale(34.0));
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 82.0, 116.0);
        self.draw_center_at(&actions, 147.0, 151.0, 196.0);
        self.draw_face_at(
            &actions,
            &[
                (BindingAction::Y, 238.0, 91.0),
                (BindingAction::X, 275.0, 91.0),
                (BindingAction::R, 312.0, 91.0),
                (BindingAction::L, 238.0, 132.0),
                (BindingAction::B, 275.0, 132.0),
                (BindingAction::A, 312.0, 132.0),
            ],
        );
    }

    fn draw_pce_two(&mut self) {
        self.title(self.kind.label());
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 112.0),
            egui::vec2(self.scale(332.0), self.scale(112.0)),
        );
        self.shell(body, self.scale(13.0));
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 79.0, 112.0);
        self.draw_face_at(
            &actions,
            &[
                (BindingAction::B, 272.0, 128.0),
                (BindingAction::A, 307.0, 111.0),
            ],
        );
        self.draw_center_at(&actions, 130.0, 153.0, 207.0);
    }

    fn draw_face_at(&mut self, actions: &[DiagramAction], positions: &[(BindingAction, f32, f32)]) {
        for &(action, x, y) in positions {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_circle(
                    self.pos(x, y),
                    self.scale(16.0),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_center_at(&mut self, actions: &[DiagramAction], y: f32, select_x: f32, start_x: f32) {
        for (action, x) in [
            (BindingAction::Select, select_x),
            (BindingAction::Start, start_x),
        ] {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_rect(
                    egui::Rect::from_center_size(
                        self.pos(x, y),
                        egui::vec2(self.scale(43.0), self.scale(18.0)),
                    ),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_dpad(&mut self, actions: &[DiagramAction]) {
        for (action, x, y) in [
            (BindingAction::Up, 86.0, 92.0),
            (BindingAction::Left, 62.0, 116.0),
            (BindingAction::Right, 110.0, 116.0),
            (BindingAction::Down, 86.0, 140.0),
        ] {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_rect(
                    egui::Rect::from_center_size(
                        self.pos(x, y),
                        egui::vec2(self.scale(28.0), self.scale(28.0)),
                    ),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_face_buttons(&mut self, actions: &[DiagramAction]) {
        let positions = if self.kind == DiagramKind::StandardGamepad {
            vec![
                (BindingAction::X, 282.0, 92.0),
                (BindingAction::Y, 255.0, 120.0),
                (BindingAction::A, 309.0, 120.0),
                (BindingAction::B, 282.0, 148.0),
            ]
        } else if self.kind == DiagramKind::PceSixButton {
            vec![
                (BindingAction::Y, 244.0, 87.0),
                (BindingAction::X, 279.0, 87.0),
                (BindingAction::L, 244.0, 123.0),
                (BindingAction::R, 279.0, 123.0),
                (BindingAction::B, 244.0, 159.0),
                (BindingAction::A, 279.0, 159.0),
            ]
        } else {
            vec![
                (BindingAction::Y, 255.0, 94.0),
                (BindingAction::X, 285.0, 112.0),
                (BindingAction::B, 255.0, 140.0),
                (BindingAction::A, 285.0, 140.0),
            ]
        };
        for (action, x, y) in positions {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_circle(
                    self.pos(x, y),
                    self.scale(15.0),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_center_buttons(&mut self, actions: &[DiagramAction]) {
        for (action, x) in [
            (BindingAction::Select, 154.0),
            (BindingAction::Start, 206.0),
        ] {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_rect(
                    egui::Rect::from_center_size(
                        self.pos(x, 145.0),
                        egui::vec2(self.scale(42.0), self.scale(17.0)),
                    ),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_coleco(&mut self) {
        self.title("ColecoVision controller");
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 170.0),
            egui::vec2(self.scale(206.0), self.scale(278.0)),
        );
        self.shell(body, self.scale(20.0));
        let actions = self.kind.actions();
        self.draw_dpad_at(&actions, 180.0, 78.0);
        self.control_circle(
            self.pos(112.0, 135.0),
            self.scale(14.0),
            DiagramAction::Joypad(BindingAction::A),
        );
        self.control_circle(
            self.pos(248.0, 135.0),
            self.scale(14.0),
            DiagramAction::Joypad(BindingAction::B),
        );
        self.painter.text(
            self.pos(180.0, 157.0),
            egui::Align2::CENTER_CENTER,
            "Keypad",
            egui::FontId::proportional(12.0),
            self.palette.text,
        );
        for row in 0..4 {
            for column in 0..3 {
                let key = egui::Rect::from_center_size(
                    self.pos(151.0 + column as f32 * 29.0, 183.0 + row as f32 * 25.0),
                    egui::vec2(self.scale(22.0), self.scale(18.0)),
                );
                let label = if row == 3 {
                    ["*", "0", "#"][column].to_owned()
                } else {
                    (row * 3 + column + 1).to_string()
                };
                let action = match (row, column) {
                    (0, 0) => Some(DiagramAction::Joypad(BindingAction::Select)),
                    (0, 1) => Some(DiagramAction::Joypad(BindingAction::Start)),
                    _ => None,
                };
                if let Some(action) = action {
                    self.control_rect(key, action);
                } else {
                    self.painter
                        .rect_filled(key, self.scale(3.0), self.palette.control);
                    self.painter.text(
                        key.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(12.0),
                        self.palette.text,
                    );
                }
            }
        }
        self.painter.text(
            self.pos(180.0, 289.0),
            egui::Align2::CENTER_CENTER,
            "Keys 1 and 2 use the current aliases",
            egui::FontId::proportional(12.0),
            self.palette.text,
        );
    }

    fn draw_dpad_at(&mut self, actions: &[DiagramAction], x: f32, y: f32) {
        let hub = egui::Rect::from_center_size(
            self.pos(x, y),
            egui::vec2(self.scale(26.0), self.scale(26.0)),
        );
        self.painter.rect(
            hub,
            self.scale(3.0),
            self.palette.control,
            egui::Stroke::new(1.0, self.palette.outline),
            egui::StrokeKind::Inside,
        );
        for (action, dx, dy) in [
            (BindingAction::Up, 0.0, -24.0),
            (BindingAction::Left, -24.0, 0.0),
            (BindingAction::Right, 24.0, 0.0),
            (BindingAction::Down, 0.0, 24.0),
        ] {
            if actions.contains(&DiagramAction::Joypad(action)) {
                self.control_rect(
                    egui::Rect::from_center_size(
                        self.pos(x + dx, y + dy),
                        egui::vec2(self.scale(28.0), self.scale(28.0)),
                    ),
                    DiagramAction::Joypad(action),
                );
            }
        }
    }

    fn draw_wonderswan(&mut self) {
        self.title("WonderSwan direct controls");
        let body = egui::Rect::from_center_size(
            self.pos(180.0, 112.0),
            egui::vec2(self.scale(326.0), self.scale(145.0)),
        );
        self.shell(body, self.scale(28.0));
        self.ws_cluster(
            73.0,
            78.0,
            [
                WonderSwanButton::Y1,
                WonderSwanButton::Y2,
                WonderSwanButton::Y3,
                WonderSwanButton::Y4,
            ],
        );
        self.ws_cluster(
            73.0,
            145.0,
            [
                WonderSwanButton::X1,
                WonderSwanButton::X2,
                WonderSwanButton::X3,
                WonderSwanButton::X4,
            ],
        );
        let screen = egui::Rect::from_center_size(
            self.pos(183.0, 111.0),
            egui::vec2(self.scale(92.0), self.scale(53.0)),
        );
        self.screen(screen);
        self.control_circle(
            self.pos(280.0, 100.0),
            self.scale(15.0),
            DiagramAction::WonderSwan(WonderSwanButton::A),
        );
        self.control_circle(
            self.pos(311.0, 123.0),
            self.scale(15.0),
            DiagramAction::WonderSwan(WonderSwanButton::B),
        );
        self.control_rect(
            egui::Rect::from_center_size(
                self.pos(183.0, 151.0),
                egui::vec2(self.scale(50.0), self.scale(18.0)),
            ),
            DiagramAction::WonderSwan(WonderSwanButton::Start),
        );
    }

    fn ws_cluster(&mut self, x: f32, y: f32, buttons: [WonderSwanButton; 4]) {
        for (button, dx, dy) in [
            (buttons[0], 0.0, -24.0),
            (buttons[3], -24.0, 0.0),
            (buttons[1], 24.0, 0.0),
            (buttons[2], 0.0, 24.0),
        ] {
            self.control_rect(
                egui::Rect::from_center_size(
                    self.pos(x + dx, y + dy),
                    egui::vec2(self.scale(27.0), self.scale(27.0)),
                ),
                DiagramAction::WonderSwan(button),
            );
        }
    }

    fn control_rect(&mut self, rect: egui::Rect, action: DiagramAction) {
        let id = self
            .ui
            .id()
            .with(("controller_diagram", self.kind, action_id(action)));
        let response = self.ui.interact(rect, id, egui::Sense::click());
        let fill = self.fill(action);
        self.painter.rect_filled(rect, self.scale(5.0), fill);
        self.painter.rect_stroke(
            rect,
            self.scale(5.0),
            egui::Stroke::new(1.0, self.palette.outline),
            egui::StrokeKind::Inside,
        );
        if response.hovered() {
            self.painter.rect_stroke(
                rect,
                self.scale(5.0),
                egui::Stroke::new(1.5, self.palette.accent),
                egui::StrokeKind::Outside,
            );
        }
        if let Some(direction) = direction_vector(action) {
            let arrow = direction * self.scale(13.0);
            self.painter.arrow(
                rect.center() - arrow * 0.5,
                arrow,
                egui::Stroke::new(1.8, self.control_text_color(action)),
            );
        } else {
            self.painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.control_label(action),
                egui::FontId::proportional(12.0),
                self.control_text_color(action),
            );
        }
        if response.clicked() {
            self.clicked = Some(action);
        }
    }

    fn control_circle(&mut self, center: egui::Pos2, radius: f32, action: DiagramAction) {
        let rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
        let id = self
            .ui
            .id()
            .with(("controller_diagram", self.kind, action_id(action)));
        let response = self.ui.interact(rect, id, egui::Sense::click());
        self.painter
            .circle_filled(center, radius, self.fill(action));
        self.painter
            .circle_stroke(center, radius, egui::Stroke::new(1.0, self.palette.outline));
        if response.hovered() {
            self.painter
                .circle_stroke(center, radius, egui::Stroke::new(1.5, self.palette.accent));
        }
        self.painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            self.control_label(action),
            egui::FontId::proportional(12.0),
            self.control_text_color(action),
        );
        if response.clicked() {
            self.clicked = Some(action);
        }
    }

    fn fill(&self, action: DiagramAction) -> egui::Color32 {
        if self.selected == Some(action) {
            self.palette.selected
        } else if is_pressed(action, self.pressed_host_mask) {
            self.palette.accent
        } else {
            self.palette.control
        }
    }

    fn control_label(&self, action: DiagramAction) -> &'static str {
        match (self.kind, action) {
            (DiagramKind::Coleco, DiagramAction::Joypad(BindingAction::A)) => "L",
            (DiagramKind::Coleco, DiagramAction::Joypad(BindingAction::B)) => "R",
            (DiagramKind::Coleco, DiagramAction::Joypad(BindingAction::Select)) => "1",
            (DiagramKind::Coleco, DiagramAction::Joypad(BindingAction::Start)) => "2",
            (_, DiagramAction::WonderSwan(WonderSwanButton::X1)) => "X1",
            (_, DiagramAction::WonderSwan(WonderSwanButton::X2)) => "X2",
            (_, DiagramAction::WonderSwan(WonderSwanButton::X3)) => "X3",
            (_, DiagramAction::WonderSwan(WonderSwanButton::X4)) => "X4",
            (_, DiagramAction::WonderSwan(WonderSwanButton::Y1)) => "Y1",
            (_, DiagramAction::WonderSwan(WonderSwanButton::Y2)) => "Y2",
            (_, DiagramAction::WonderSwan(WonderSwanButton::Y3)) => "Y3",
            (_, DiagramAction::WonderSwan(WonderSwanButton::Y4)) => "Y4",
            _ => self.kind.action_label(action),
        }
    }

    fn control_text_color(&self, action: DiagramAction) -> egui::Color32 {
        if self.selected == Some(action) {
            egui::Color32::from_rgb(35, 30, 20)
        } else {
            self.palette.text
        }
    }
}

fn is_pressed(action: DiagramAction, pressed_host_mask: u16) -> bool {
    let DiagramAction::Joypad(action) = action else {
        return false;
    };
    let button = match action {
        BindingAction::Right => HostButton::Right,
        BindingAction::Left => HostButton::Left,
        BindingAction::Up => HostButton::Up,
        BindingAction::Down => HostButton::Down,
        BindingAction::A => HostButton::A,
        BindingAction::B => HostButton::B,
        BindingAction::X => HostButton::X,
        BindingAction::Y => HostButton::Y,
        BindingAction::L => HostButton::L,
        BindingAction::R => HostButton::R,
        BindingAction::Start => HostButton::Start,
        BindingAction::Select => HostButton::Select,
    };
    pressed_host_mask & button.host_mask_bit() != 0
}

fn direction_vector(action: DiagramAction) -> Option<egui::Vec2> {
    match action {
        DiagramAction::Joypad(BindingAction::Up) => Some(egui::vec2(0.0, -1.0)),
        DiagramAction::Joypad(BindingAction::Down) => Some(egui::vec2(0.0, 1.0)),
        DiagramAction::Joypad(BindingAction::Left) => Some(egui::vec2(-1.0, 0.0)),
        DiagramAction::Joypad(BindingAction::Right) => Some(egui::vec2(1.0, 0.0)),
        _ => None,
    }
}

const fn action_id(action: DiagramAction) -> u8 {
    match action {
        DiagramAction::Joypad(BindingAction::Right) => 0,
        DiagramAction::Joypad(BindingAction::Left) => 1,
        DiagramAction::Joypad(BindingAction::Up) => 2,
        DiagramAction::Joypad(BindingAction::Down) => 3,
        DiagramAction::Joypad(BindingAction::A) => 4,
        DiagramAction::Joypad(BindingAction::B) => 5,
        DiagramAction::Joypad(BindingAction::X) => 6,
        DiagramAction::Joypad(BindingAction::Y) => 7,
        DiagramAction::Joypad(BindingAction::Select) => 8,
        DiagramAction::Joypad(BindingAction::Start) => 9,
        DiagramAction::Joypad(BindingAction::L) => 10,
        DiagramAction::Joypad(BindingAction::R) => 11,
        DiagramAction::WonderSwan(WonderSwanButton::X1) => 12,
        DiagramAction::WonderSwan(WonderSwanButton::X2) => 13,
        DiagramAction::WonderSwan(WonderSwanButton::X3) => 14,
        DiagramAction::WonderSwan(WonderSwanButton::X4) => 15,
        DiagramAction::WonderSwan(WonderSwanButton::Y1) => 16,
        DiagramAction::WonderSwan(WonderSwanButton::Y2) => 17,
        DiagramAction::WonderSwan(WonderSwanButton::Y3) => 18,
        DiagramAction::WonderSwan(WonderSwanButton::Y4) => 19,
        DiagramAction::WonderSwan(WonderSwanButton::A) => 20,
        DiagramAction::WonderSwan(WonderSwanButton::B) => 21,
        DiagramAction::WonderSwan(WonderSwanButton::Start) => 22,
    }
}
