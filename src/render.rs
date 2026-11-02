use std::sync::Arc;

use egui::epaint::ClippedShape;
use winit::window::Window;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("surface creation failed: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("no suitable adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("device request failed: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("surface has no default configuration")]
    NoDefaultConfig,
    #[error("surface validation error")]
    SurfaceValidation,
}

pub struct SurfaceFrame {
    texture: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
}

pub struct EncodedFrame {
    encoder: wgpu::CommandBuffer,
    user: Vec<wgpu::CommandBuffer>,
}

pub struct Renderer {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_renderer: egui_wgpu::Renderer,
    submit_buffers: Vec<wgpu::CommandBuffer>,
    pending_free: Vec<egui::TextureId>,
    vsync: bool,
    zero_sized: bool,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("client"),
                ..Default::default()
            }))?;

        let size = window.inner_size();
        let zero_sized = size.width == 0 || size.height == 0;
        let width = size.width.max(1);
        let height = size.height.max(1);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or(RenderError::NoDefaultConfig)?;
        let vsync = false;
        config.present_mode = pick_present_mode(&surface, &adapter, vsync);
        surface.configure(&device, &config);

        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            _instance: instance,
            surface,
            adapter,
            device,
            queue,
            config,
            egui_renderer,
            submit_buffers: Vec::with_capacity(8),
            pending_free: Vec::with_capacity(8),
            vsync,
            zero_sized,
        })
    }

    pub fn vsync(&self) -> bool {
        self.vsync
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.zero_sized = true;
            return;
        }
        self.zero_sized = false;
        self.config.width = width;
        self.config.height = height;
        self.reconfigure();
    }

    pub fn set_vsync(&mut self, vsync: bool) {
        if self.vsync == vsync {
            return;
        }
        self.vsync = vsync;
        let mode = pick_present_mode(&self.surface, &self.adapter, vsync);
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.reconfigure();
        }
    }

    pub fn acquire(&mut self) -> Result<Option<SurfaceFrame>, RenderError> {
        if self.zero_sized {
            return Ok(None);
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                Ok(Some(SurfaceFrame { texture, view }))
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.reconfigure();
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => Err(RenderError::SurfaceValidation),
        }
    }

    pub fn upload_textures(&mut self, mut textures: egui::TexturesDelta) {
        for id in self.pending_free.drain(..) {
            self.egui_renderer.free_texture(&id);
        }
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        self.pending_free.extend(textures.free.iter().copied());
        textures.clear();
    }

    pub fn encode(
        &mut self,
        frame: &SurfaceFrame,
        ctx: &egui::Context,
        shapes: Vec<ClippedShape>,
        pixels_per_point: f32,
    ) -> EncodedFrame {
        let paint_jobs = ctx.tessellate(shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let user = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            let attachment = wgpu::RenderPassColorAttachment {
                view: &frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.06,
                        b: 0.09,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("main"),
                    color_attachments: &[Some(attachment)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &paint_jobs, &screen);
        }

        EncodedFrame {
            encoder: encoder.finish(),
            user,
        }
    }

    pub fn submit(&mut self, frame: SurfaceFrame, encoded: EncodedFrame) {
        self.submit_buffers.clear();
        self.submit_buffers.extend(encoded.user);
        self.submit_buffers.push(encoded.encoder);
        self.queue.submit(self.submit_buffers.drain(..));
        self.queue.present(frame.texture);
    }

    fn reconfigure(&mut self) {
        if self.zero_sized {
            return;
        }
        self.surface.configure(&self.device, &self.config);
    }
}

fn pick_present_mode(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    vsync: bool,
) -> wgpu::PresentMode {
    if vsync {
        return wgpu::PresentMode::Fifo;
    }
    let caps = surface.get_capabilities(adapter);
    [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
        .into_iter()
        .find(|mode| caps.present_modes.contains(mode))
        .unwrap_or(wgpu::PresentMode::Fifo)
}
