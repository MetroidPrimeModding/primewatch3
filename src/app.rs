//! Application shell: winit event loop + wgpu device/surface + a single egui window.
//!
//! Ports the window/context/defs-load parts of `../primewatch2/src/PrimeWatch.cpp`
//! (`initAndCreateWindow`, `initGlAndImgui`, `mainLoop`, `doFrame`, `framebuffer_size_cb`).
//! Game-specific pieces (memory attach, world renderer, inspector) belong to later phases.

use std::error::Error;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::scene::SpikeScene;
use crate::structs::prime_structs::GameStructs;

/// Build the event loop and run the app. Mirrors `main()` in the C++ entrypoint.
pub fn run() -> Result<(), Box<dyn Error>> {
  let event_loop = EventLoop::new()?;
  let mut app = App::new();
  event_loop.run_app(&mut app)?;
  Ok(())
}

/// Owns the long-lived game state plus the render state that only exists while the
/// window is active. No globals — everything is threaded explicitly (CLAUDE.md).
struct App {
  /// Local MEM1 snapshot, refreshed each frame from `dolphin` (P3.2).
  mem: GameMemory,
  /// Live Dolphin process attachment (P2 / P3.2).
  dolphin: DolphinMemoryAccess,
  #[allow(dead_code)] // consumed by the inspector in Phase 7
  structs: GameStructs,
  /// Whether the `.bs` definitions loaded — drives which egui window is shown,
  /// mirroring `GameDefinitions::isLoaded()` in C++ `doFrame`.
  defs_loaded: bool,
  /// Either "Loaded N structs and M enums" or the load error string.
  status_text: String,
  /// Render state — `None` until `resumed` (Wayland/macOS require deferred creation).
  window: Option<AppWindow>,
}

impl App {
  fn new() -> Self {
    let mut mem = GameMemory::new();
    let mut dolphin = DolphinMemoryAccess::new();

    let mut structs = GameStructs::new_empty();
    let load_result = structs.load_from_dir("prime_defs");
    let (defs_loaded, status_text) = match load_result {
      Ok(()) => {
        let text = format!(
          "Loaded {} structs and {} enums",
          structs.structs.len(),
          structs.enums.len()
        );
        println!("{text}");
        (true, text)
      }
      Err(err) => {
        println!("Error loading structs: {err}");
        (false, err)
      }
    };

    // Offline dump path (C++ `PrimeWatch::initGlAndImgui`, `PrimeWatch.cpp:99-103`):
    // auto-load `./mem1.raw` when it sits next to the binary. A later live memcpy
    // simply overwrites it; a missing/short file is not fatal.
    if std::path::Path::new("./mem1.raw").exists() {
      match mem.load_from_file("./mem1.raw") {
        Ok(()) => println!("Loaded ./mem1.raw"),
        Err(err) => eprintln!("Failed to load ./mem1.raw: {err}"),
      }
    }

    // Auto-attach only when exactly one Dolphin is running (C++
    // `PrimeWatch::initAndCreateWindow`, `PrimeWatch.cpp:66-70`).
    let pids = dolphin.get_dolphin_pids();
    if pids.len() == 1 {
      let pid = pids[0].as_u32() as i32;
      if dolphin.attach_to_process(pid) {
        println!("Attached to Dolphin pid {pid}");
      } else {
        eprintln!("Failed to attach to Dolphin pid {pid}");
      }
    } else if pids.len() > 1 {
      println!("{} Dolphin processes found; not auto-attaching", pids.len());
    }

    Self {
      mem,
      dolphin,
      structs,
      defs_loaded,
      status_text,
      window: None,
    }
  }
}

impl ApplicationHandler for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    if self.window.is_some() {
      return;
    }
    match AppWindow::new(event_loop) {
      Ok(window) => {
        window.window.request_redraw();
        self.window = Some(window);
      }
      Err(err) => {
        eprintln!("Failed to create window: {err}");
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
    let Some(window) = self.window.as_mut() else {
      return;
    };

    let _ = window.egui_state.on_window_event(&window.window, &event);

    match event {
      WindowEvent::CloseRequested => event_loop.exit(),
      WindowEvent::Resized(size) => window.resize(size),
      WindowEvent::ScaleFactorChanged { .. } => window.window.request_redraw(),
      WindowEvent::RedrawRequested => {
        // Per-frame snapshot refresh (C++ `PrimeWatch::doMemoryParse`,
        // `PrimeWatch.cpp:483-488`): gated on the defs being loaded; a no-op
        // while detached.
        // TODO(P9.1): the entities parse (`GameObjectUtils`) slots in right here,
        // and input -> parse -> ui -> render gets its real ordering.
        if self.defs_loaded {
          self.mem.update_from_dolphin(&self.dolphin);
        }
        window.render(self.defs_loaded, &self.status_text);
      }
      _ => {}
    }
  }

  fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
    // TODO(P9): only redraw on demand / frame-pace instead of spinning.
    if let Some(window) = self.window.as_ref() {
      window.window.request_redraw();
    }
  }
}

/// wgpu + egui render state. Created in `resumed`, dropped when the app exits.
struct AppWindow {
  window: Arc<Window>,
  surface: wgpu::Surface<'static>,
  device: wgpu::Device,
  queue: wgpu::Queue,
  config: wgpu::SurfaceConfiguration,
  egui_ctx: egui::Context,
  egui_state: egui_winit::State,
  egui_renderer: egui_wgpu::Renderer,
  /// P1.3 spike: rotating cube rendered to an offscreen texture, composited into
  /// the egui UI as an `egui::Image` (see the "Chosen pattern B" note in TASKS.md).
  scene: SpikeScene,
  /// egui user-texture id for `scene`'s colour target. `None` until the first
  /// `register_native_texture`; reused via `update_egui_texture_from_wgpu_texture`
  /// thereafter (that call rebuilds the bind group, so it survives target resize).
  scene_texture: Option<egui::TextureId>,
}

impl AppWindow {
  fn new(event_loop: &ActiveEventLoop) -> Result<Self, Box<dyn Error>> {
    let window = Arc::new(
      event_loop.create_window(
        Window::default_attributes()
          .with_title("Prime Watch 2")
          .with_inner_size(LogicalSize::new(1200, 800)),
      )?,
    );

    let instance = wgpu::Instance::default();
    let surface = instance.create_surface(window.clone())?;

    let (adapter, device, queue) = pollster::block_on(async {
      let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
          compatible_surface: Some(&surface),
          ..Default::default()
        })
        .await?;
      let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await?;
      Ok::<_, Box<dyn Error>>((adapter, device, queue))
    })?;

    let size = window.inner_size();
    let mut config = surface
      .get_default_config(&adapter, size.width.max(1), size.height.max(1))
      .ok_or("surface is not supported by the adapter")?;
    config.present_mode = wgpu::PresentMode::Fifo;
    if let Some(srgb) = surface
      .get_capabilities(&adapter)
      .formats
      .iter()
      .copied()
      .find(|f| f.is_srgb())
    {
      config.format = srgb;
    }
    surface.configure(&device, &config);

    let egui_ctx = egui::Context::default();
    let egui_state = egui_winit::State::new(
      egui_ctx.clone(),
      egui::ViewportId::ROOT,
      &*window,
      Some(window.scale_factor() as f32),
      None,
      Some(device.limits().max_texture_dimension_2d as usize),
    );
    let egui_renderer = egui_wgpu::Renderer::new(
      &device,
      config.format,
      egui_wgpu::RendererOptions::default(),
    );

    let scene = SpikeScene::new(&device, (800, 600));

    Ok(Self {
      window,
      surface,
      device,
      queue,
      config,
      egui_ctx,
      egui_state,
      egui_renderer,
      scene,
      scene_texture: None,
    })
  }

  /// Reconfigure the swapchain on window resize (C++ `framebuffer_size_cb`).
  fn resize(&mut self, size: PhysicalSize<u32>) {
    if size.width > 0 && size.height > 0 {
      self.config.width = size.width;
      self.config.height = size.height;
      self.surface.configure(&self.device, &self.config);
    }
    self.window.request_redraw();
  }

  /// One frame: build the egui UI, clear to black, paint egui (C++ `doFrame`).
  fn render(&mut self, defs_loaded: bool, status_text: &str) {
    let frame = match self.surface.get_current_texture() {
      wgpu::CurrentSurfaceTexture::Success(frame)
      | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
      wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
        self.surface.configure(&self.device, &self.config);
        return;
      }
      wgpu::CurrentSurfaceTexture::Timeout
      | wgpu::CurrentSurfaceTexture::Occluded
      | wgpu::CurrentSurfaceTexture::Validation => return,
    };

    let view = frame
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("primewatch"),
      });

    // 3D scene first, into its own offscreen target (C++ `doFrame` renders the
    // world before the egui draw data). Separate pass, same encoder + submit.
    self.scene.render(&self.queue, &mut encoder);

    // Register (first use / after resize) or reuse the egui user texture that
    // wraps the scene's colour target.
    let scene_texture = match self.scene_texture {
      Some(id) => {
        self.egui_renderer.update_egui_texture_from_wgpu_texture(
          &self.device,
          self.scene.color_view(),
          wgpu::FilterMode::Linear,
          id,
        );
        id
      }
      None => {
        let id = self.egui_renderer.register_native_texture(
          &self.device,
          self.scene.color_view(),
          wgpu::FilterMode::Linear,
        );
        self.scene_texture = Some(id);
        id
      }
    };

    let raw_input = self.egui_state.take_egui_input(&self.window);
    self.egui_ctx.begin_pass(raw_input);
    // Single window either way, matching the C++ "NOT LOADED" fallback in `doFrame`.
    let title = if defs_loaded {
      "Prime Watch"
    } else {
      "NOT LOADED"
    };
    egui::Window::new(title)
      .resizable(false)
      .collapsible(false)
      .show(&self.egui_ctx, |ui| {
        if defs_loaded {
          ui.label(status_text);
        } else {
          ui.label("Script definitions are not currently loaded.");
          ui.label("These are required to function.");
          ui.label("Current error:");
          ui.label(status_text);
        }
      });

    // P1.3 spike: show the offscreen 3D target. The panel's available size drives
    // next frame's scene target size (documented one-frame lag).
    let mut world_view_size_pts: Option<egui::Vec2> = None;
    egui::Window::new("World")
      .default_size([640.0, 480.0])
      .show(&self.egui_ctx, |ui| {
        let avail = ui.available_size();
        world_view_size_pts = Some(avail);
        ui.image(egui::load::SizedTexture::new(scene_texture, avail));
      });

    let full_output = self.egui_ctx.end_pass();

    self
      .egui_state
      .handle_platform_output(&self.window, full_output.platform_output);

    let tris = self
      .egui_ctx
      .tessellate(full_output.shapes, full_output.pixels_per_point);

    for (id, deltas) in &full_output.textures_delta.set {
      for delta in deltas {
        self
          .egui_renderer
          .update_texture(&self.device, &self.queue, *id, delta);
      }
    }

    let screen_desc = egui_wgpu::ScreenDescriptor {
      size_in_pixels: [self.config.width, self.config.height],
      pixels_per_point: full_output.pixels_per_point,
    };
    let user_cmd_bufs = self.egui_renderer.update_buffers(
      &self.device,
      &self.queue,
      &mut encoder,
      &tris,
      &screen_desc,
    );

    {
      let mut pass = encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("egui"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
              // == C++ glClearColor(0, 0, 0, 1)
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          timestamp_writes: None,
          occlusion_query_set: None,
          multiview_mask: None,
        })
        .forget_lifetime();
      self.egui_renderer.render(&mut pass, &tris, &screen_desc);
    }

    for id in &full_output.textures_delta.free {
      self.egui_renderer.free_texture(id);
    }

    self.queue.submit(
      user_cmd_bufs
        .into_iter()
        .chain(std::iter::once(encoder.finish())),
    );
    self.window.pre_present_notify();
    self.queue.present(frame);

    // Resize the offscreen target to match the "World" panel for the next frame
    // (documented one-frame lag). `SpikeScene::resize` reports whether the target
    // was recreated; `AppWindow` doesn't need to act on it because
    // `update_egui_texture_from_wgpu_texture` rebuilds the egui bind group from
    // the current view every frame anyway (no re-`register` / no leak).
    if let Some(sz) = world_view_size_pts {
      let ppp = full_output.pixels_per_point;
      let w = (sz.x * ppp).round().max(1.0) as u32;
      let h = (sz.y * ppp).round().max(1.0) as u32;
      let _recreated = self.scene.resize(&self.device, (w, h));
    }
  }
}
