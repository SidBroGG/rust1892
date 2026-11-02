use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::frame::{self, FixedStep, FramePacer};
use crate::input;
use crate::metrics::{Metrics, MetricsSnapshot, STAGE_COUNT, Stage};
use crate::render::Renderer;
use crate::ui::{self, FrameSettings, OverlayAction, OverlayState};

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    fixed: FixedStep,
    pacer: FramePacer,
    metrics: Metrics,
    snapshot: MetricsSnapshot,
    overlay: OverlayState,
    settings: FrameSettings,
    last_frame: Instant,
    ticks: u64,
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            fixed: FixedStep::default(),
            pacer: FramePacer::new(0),
            metrics: Metrics::new(),
            snapshot: MetricsSnapshot::default(),
            overlay: OverlayState::default(),
            settings: FrameSettings::default(),
            last_frame: Instant::now(),
            ticks: 0,
            error: None,
        }
    }

    pub fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }

    fn init_graphics(&mut self, window: Arc<Window>) {
        let renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.error = Some(anyhow::Error::new(error));
                return;
            }
        };
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        self.settings.vsync = renderer.vsync();
        self.renderer = Some(renderer);
        self.egui_state = Some(egui_state);
        self.window = Some(window);
        self.last_frame = Instant::now();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.renderer.is_none() || self.egui_state.is_none() {
            return;
        }

        let frame_start = Instant::now();
        self.pacer.begin_frame(frame_start);

        let input_start = Instant::now();
        let raw_input = match self.egui_state.as_mut() {
            Some(state) => state.take_egui_input(&window),
            None => return,
        };
        let mut stage = [0.0f32; STAGE_COUNT];
        stage[Stage::Input.index()] = elapsed_ms(input_start);

        let snapshot = self.snapshot;
        let mut action = OverlayAction::default();
        let full_output = self.egui_ctx.run_pass(raw_input, |ctx| {
            action = ui::overlay(
                ctx,
                &mut self.overlay,
                &self.settings,
                &snapshot,
                self.ticks,
            );
        });
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = full_output;
        if let Some(state) = self.egui_state.as_mut() {
            state.handle_platform_output(&window, platform_output);
        }

        let elapsed = frame_start.saturating_duration_since(self.last_frame);
        self.last_frame = frame_start;
        let step = self.fixed.advance(elapsed);
        let _alpha = step.alpha;
        let tick_start = Instant::now();
        for _ in 0..step.steps {
            self.ticks += 1;
        }
        stage[Stage::Tick.index()] = elapsed_ms(tick_start);

        if let Some(renderer) = self.renderer.as_mut() {
            renderer.upload_textures(textures_delta);
            match renderer.acquire() {
                Ok(Some(frame)) => {
                    let encode_start = Instant::now();
                    let encoded = renderer.encode(&frame, &self.egui_ctx, shapes, pixels_per_point);
                    stage[Stage::RenderEncode.index()] = elapsed_ms(encode_start);
                    let submit_start = Instant::now();
                    renderer.submit(frame, encoded);
                    stage[Stage::SubmitPresent.index()] = elapsed_ms(submit_start);
                }
                Ok(None) => {}
                Err(error) => {
                    self.error = Some(anyhow::Error::new(error));
                    event_loop.exit();
                    return;
                }
            }
        }

        let pace_start = Instant::now();
        let wait = self.pacer.wait_time(pace_start);
        if !wait.is_zero() {
            frame::wait_precise(wait);
        }
        stage[Stage::PaceWait.index()] = elapsed_ms(pace_start);

        self.metrics.record_frame(elapsed_ms(frame_start), stage);
        self.snapshot = self.metrics.snapshot();

        self.apply_action(action);
    }

    fn apply_action(&mut self, action: OverlayAction) {
        if let Some(vsync) = action.set_vsync {
            self.settings.vsync = vsync;
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.set_vsync(vsync);
            }
        }
        if let Some(limit) = action.set_fps_limit {
            self.settings.fps_limit = limit;
            self.pacer.set_limit(limit);
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Client")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let window = Arc::new(window);
                self.init_graphics(window.clone());
                if self.error.is_some() {
                    event_loop.exit();
                } else {
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.error = Some(anyhow::Error::new(error));
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let response = self
            .egui_state
            .as_mut()
            .map(|state| state.on_window_event(&window, &event));
        let consumed = response.as_ref().is_some_and(|response| response.consumed);
        let repaint = response.as_ref().is_some_and(|response| response.repaint);
        if repaint {
            window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.egui_ctx.set_pixels_per_point(scale_factor as f32);
                if let Some(renderer) = self.renderer.as_mut() {
                    let size = window.inner_size();
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !consumed && input::is_overlay_toggle(&event) {
                    self.overlay.visible = !self.overlay.visible;
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn elapsed_ms(start: Instant) -> f32 {
    start.elapsed().as_secs_f32() * 1000.0
}
