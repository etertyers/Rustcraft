//! The window the first-launch screen runs in: its own small winit app and
//! egui-on-wgpu renderer, run to the end before the game's window exists.
//! Done in the same event loop (`run_app_on_demand`), so the game then starts
//! without restarting the process.

use super::screen::{Outcome, SetupScreen};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{Window, WindowId};

const FRAME: Duration = Duration::from_millis(33);

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("setup device"),
                ..Default::default()
            }))?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("the surface isn't supported by the GPU")?;
        let caps = surface.get_capabilities(&adapter);
        if let Some(format) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *format;
        }
        config.view_formats = vec![config.format.remove_srgb_suffix()];
        config.usage |= wgpu::TextureUsages::COPY_SRC;
        surface.configure(&device, &config);
        let renderer = egui_wgpu::Renderer::new(
            &device,
            config.format.remove_srgb_suffix(),
            egui_wgpu::RendererOptions::default(),
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
        })
    }

    fn copy_out(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> (wgpu::Buffer, u32) {
        let padded = (self.config.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("setup capture"),
            size: u64::from(padded) * u64::from(self.config.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.config.height),
                },
            },
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
        );
        (buffer, padded)
    }

    fn save(&self, buffer: &wgpu::Buffer, padded: u32, path: &std::path::Path) {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        if self
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .is_err()
        {
            return;
        }
        let Ok(data) = slice.get_mapped_range() else {
            return;
        };
        let (w, h) = (self.config.width as usize, self.config.height as usize);
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            for px in data[row * padded as usize..][..w * 4].as_chunks::<4>().0 {
                if bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(data);
        buffer.unmap();
        match image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
            Some(image) => match image.save(path) {
                Ok(()) => log::info!("first-launch screen saved to {}", path.display()),
                Err(err) => log::error!("can't save {}: {err}", path.display()),
            },
            None => log::error!("capture had the wrong size"),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Draws the frame; with `capture` also saves it as a PNG, drawn into a
    /// texture of its own so a hidden window still gives a picture.
    fn draw(
        &mut self,
        output: egui::FullOutput,
        ctx: &egui::Context,
        capture: Option<&std::path::Path>,
    ) {
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        if let Some(path) = capture {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("setup capture"),
                size: wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format.remove_srgb_suffix(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let extra = self.paint(&mut encoder, &view, &primitives, &screen);
            let (buffer, padded) = self.copy_out(&mut encoder, &texture);
            self.queue
                .submit(extra.into_iter().chain([encoder.finish()]));
            self.save(&buffer, padded, path);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format.remove_srgb_suffix()),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let extra = self.paint(&mut encoder, &view, &primitives, &screen);
        self.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }

    fn paint(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        primitives: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
    ) -> Vec<wgpu::CommandBuffer> {
        let extra =
            self.renderer
                .update_buffers(&self.device, &self.queue, encoder, primitives, screen);
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("setup ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.11,
                            g: 0.11,
                            b: 0.11,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        self.renderer.render(&mut pass, primitives, screen);
        drop(pass);
        extra
    }
}

struct Runner {
    screen: SetupScreen,
    ctx: egui::Context,
    state: Option<egui_winit::State>,
    // The surface must go before the window it draws to.
    gpu: Option<Gpu>,
    window: Option<Arc<Window>>,
    result: Option<Outcome>,
    error: Option<anyhow::Error>,
    next_frame: Instant,
    captured: bool,
}

impl Runner {
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window), Some(state), Some(gpu)) =
            (&self.window, self.state.as_mut(), self.gpu.as_mut())
        else {
            return;
        };
        let input = state.take_egui_input(window);
        let screen = &mut self.screen;
        let output = self.ctx.run_ui(input, |ui| screen.ui(ui));
        state.handle_platform_output(window, output.platform_output.clone());
        // `$RUSTCRAFT_SETUP_SHOT=<png>`: save the frame once, when the
        // download is a quarter done (the milestone's screenshot).
        let capture = std::env::var_os("RUSTCRAFT_SETUP_SHOT")
            .map(std::path::PathBuf::from)
            .filter(|_| !self.captured && self.screen.progress_fraction() > 0.25);
        self.captured |= capture.is_some();
        gpu.draw(output, &self.ctx, capture.as_deref());
        if let Some(outcome) = self.screen.outcome() {
            self.result = Some(outcome);
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let made = (|| -> Result<()> {
            let attrs = Window::default_attributes()
                .with_title("Rustcraft")
                .with_inner_size(LogicalSize::new(720.0, 520.0));
            let window = Arc::new(event_loop.create_window(attrs)?);
            self.state = Some(egui_winit::State::new(
                self.ctx.clone(),
                egui::ViewportId::ROOT,
                window.as_ref(),
                Some(window.scale_factor() as f32),
                None,
                None,
            ));
            self.gpu = Some(Gpu::new(window.clone())?);
            log::info!("first-launch screen: window opened");
            window.request_redraw();
            self.window = Some(window);
            Ok(())
        })();
        if let Err(err) = made {
            self.error = Some(err);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(window), Some(state)) = (&self.window, self.state.as_mut()) {
            let _ = state.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                self.result = Some(Outcome::Quit);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = &self.window else {
            return;
        };
        if self.screen.animating() {
            let now = Instant::now();
            if now >= self.next_frame {
                self.next_frame = now + FRAME;
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            // Idle screens redraw when an event arrives; a short timer
            // still lets the worker's last message through.
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME * 3));
            window.request_redraw();
        }
    }
}

/// Shows `screen` in its own window until it is done or closed.
pub fn run(event_loop: &mut EventLoop<()>, screen: SetupScreen) -> Result<Outcome> {
    let mut runner = Runner {
        screen,
        ctx: egui::Context::default(),
        state: None,
        gpu: None,
        window: None,
        result: None,
        error: None,
        next_frame: Instant::now(),
        captured: false,
    };
    event_loop.run_app_on_demand(&mut runner)?;
    if let Some(err) = runner.error.take() {
        return Err(err);
    }
    // Drop the surface before the window.
    runner.gpu = None;
    runner.state = None;
    runner.window = None;
    Ok(runner.result.unwrap_or(Outcome::Quit))
}
