use crate::metrics::{MetricsSnapshot, Stage};

const FPS_LIMITS: [u32; 6] = [0, 30, 60, 120, 144, 240];

#[derive(Clone, Debug, Default)]
pub struct FrameSettings {
    pub vsync: bool,
    pub fps_limit: u32,
}

pub struct OverlayState {
    pub visible: bool,
    pub custom_limit: String,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            visible: true,
            custom_limit: String::new(),
        }
    }
}

#[derive(Default)]
pub struct OverlayAction {
    pub set_vsync: Option<bool>,
    pub set_fps_limit: Option<u32>,
}

pub fn overlay(
    ctx: &egui::Context,
    state: &mut OverlayState,
    settings: &FrameSettings,
    snapshot: &MetricsSnapshot,
    ticks: u64,
) -> OverlayAction {
    let mut action = OverlayAction::default();
    if !state.visible {
        return action;
    }
    egui::Window::new("Frame metrics")
        .default_pos(egui::pos2(8.0, 8.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("FPS: {:.1}", snapshot.fps));
            egui::Grid::new("frametimes")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    ui.label("Frame avg");
                    ui.label(format!("{:.2} ms", snapshot.frametime_avg));
                    ui.end_row();
                    ui.label("Frame p95");
                    ui.label(format!("{:.2} ms", snapshot.frametime_p95));
                    ui.end_row();
                    ui.label("Frame p99");
                    ui.label(format!("{:.2} ms", snapshot.frametime_p99));
                    ui.end_row();
                    ui.label("Ticks");
                    ui.label(format!("{ticks}"));
                    ui.end_row();
                });
            ui.separator();
            ui.label("CPU stages");
            egui::Grid::new("stages")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for stage in Stage::ALL {
                        ui.label(stage.label());
                        ui.label(format!("{:.3} ms", snapshot.stage_avg[stage.index()]));
                        ui.end_row();
                    }
                });
            ui.separator();
            let mut vsync = settings.vsync;
            if ui.checkbox(&mut vsync, "VSync").changed() {
                action.set_vsync = Some(vsync);
            }
            let mut selected = settings.fps_limit;
            egui::ComboBox::from_label("FPS limit")
                .selected_text(limit_text(selected))
                .show_ui(ui, |ui| {
                    for limit in FPS_LIMITS {
                        ui.selectable_value(&mut selected, limit, limit_text(limit));
                    }
                });
            if selected != settings.fps_limit {
                action.set_fps_limit = Some(selected);
            }
            ui.horizontal(|ui| {
                ui.label("Custom");
                ui.add(
                    egui::TextEdit::singleline(&mut state.custom_limit)
                        .desired_width(56.0)
                        .hint_text("1-1000"),
                );
                if ui.button("Apply").clicked()
                    && let Ok(value) = state.custom_limit.trim().parse::<u32>()
                    && (1..=1000).contains(&value)
                {
                    action.set_fps_limit = Some(value);
                }
            });
        });
    action
}

fn limit_text(limit: u32) -> String {
    if limit == 0 {
        "Unlimited".to_owned()
    } else {
        format!("{limit}")
    }
}
