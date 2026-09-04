//! wgpu + egui render state: window/surface/device setup and the per-frame
//! `render` pass (menu bar, world view, inspector windows, egui paint).

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::ctx::Ctx;
use crate::mem::globals::{get_main, get_memory_card, get_state_manager, get_tweak_player};
use crate::ui_state;

use super::FrameState;
use super::input::WorldViewInput;
use super::menu_action::{MenuAction, apply_menu_action};
use super::objects_window::render_objects_window;
use super::raw_data_view::render_raw_data_view;

/// wgpu + egui render state. Created in `resumed`, dropped when the app exits.
pub(super) struct AppWindow {
  pub(super) window: Arc<Window>,
  surface: wgpu::Surface<'static>,
  device: wgpu::Device,
  queue: wgpu::Queue,
  config: wgpu::SurfaceConfiguration,
  pub(super) egui_ctx: egui::Context,
  pub(super) egui_state: egui_winit::State,
  egui_renderer: egui_wgpu::Renderer,
  pub(super) world: crate::world::renderer::WorldRenderer,
  /// egui user-texture id for `world`'s colour target. `None` until the first
  /// `register_native_texture`; reused via `update_egui_texture_from_wgpu_texture`
  /// thereafter (that call rebuilds the bind group, so it survives target resize).
  world_texture: Option<egui::TextureId>,
  /// Last frame's "World" panel size in physical pixels — fed to
  /// `WorldRenderer::update` this frame (documented one-frame lag). Seeded with
  /// the initial target size.
  pub(super) world_view_px: (u32, u32),
  /// Last frame's drag/scroll over the "World" image — fed to [`super::input::InputState::plan`]
  /// this frame (same one-frame lag as `world_view_px`).
  pub(super) world_view_input: WorldViewInput,
  /// Last time the egui UI layout was flushed to disk (see [`crate::ui_state`]).
  /// Re-saved at most once per [`ui_state::AUTOSAVE_INTERVAL`] from `render`,
  /// plus a final save in `App::exiting`.
  last_ui_save: Instant,
  /// FPS counter shown at the end of the toolbar. `fps_window_start` /
  /// `fps_window_frames` accumulate over the current one-second window;
  /// `fps_display` is the last computed value (rounded to whole fps).
  fps_window_start: Instant,
  fps_window_frames: u32,
  fps_display: u32,
}

impl AppWindow {
  pub(super) fn new(event_loop: &ActiveEventLoop) -> Result<Self, Box<dyn Error>> {
    // Recreate the window at its last saved size/position (if any) *before*
    // `ui_state::load` reinstalls the egui layout below — egui's window/area
    // positions are only meaningful relative to the viewport they were saved
    // against, so mismatching them here is what causes saved layouts to look
    // like they "moved" (egui clamps them back inside a smaller viewport).
    let geometry = ui_state::load_window_geometry();
    let size = geometry.as_ref().map(|g| g.size).unwrap_or((1200.0, 800.0));
    let mut attrs = Window::default_attributes()
      .with_title("Prime Watch 3")
      .with_inner_size(LogicalSize::new(size.0, size.1));
    if let Some(pos) = geometry.and_then(|g| g.position) {
      attrs = attrs.with_position(LogicalPosition::new(pos.0, pos.1));
    }
    let window = Arc::new(event_loop.create_window(attrs)?);

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
    // Restore window positions/sizes and collapsed/scroll state from the last run.
    ui_state::load(&egui_ctx);
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

    let world = crate::world::renderer::WorldRenderer::new(&device, (800, 600));

    Ok(Self {
      window,
      surface,
      device,
      queue,
      config,
      egui_ctx,
      egui_state,
      egui_renderer,
      world,
      world_texture: None,
      world_view_px: (800, 600),
      world_view_input: WorldViewInput::default(),
      last_ui_save: Instant::now(),
      fps_window_start: Instant::now(),
      fps_window_frames: 0,
      fps_display: 0,
    })
  }

  /// Reconfigure the swapchain on window resize
  pub(super) fn resize(&mut self, size: PhysicalSize<u32>) {
    if size.width > 0 && size.height > 0 {
      self.config.width = size.width;
      self.config.height = size.height;
      self.surface.configure(&self.device, &self.config);
    }
    self.window.request_redraw();
  }

  /// One frame: build the egui UI, render the 3D world, clear to black, paint
  /// egui. `fs` carries the game/UI state owned by [`super::App`].
  pub(super) fn render(&mut self, fs: &mut FrameState) {
    let defs_loaded = *fs.defs_loaded;

    // FPS counter: count frames, recompute at most once per second.
    self.fps_window_frames += 1;
    let fps_elapsed = self.fps_window_start.elapsed().as_secs_f32();
    if fps_elapsed >= 1.0 {
      self.fps_display = (self.fps_window_frames as f32 / fps_elapsed).round() as u32;
      self.fps_window_start = Instant::now();
      self.fps_window_frames = 0;
    }

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

    // 3D world first, into its own offscreen target. Separate pass, same encoder + submit.
    self.world.render(&self.device, &self.queue, &mut encoder);

    // Register (first use / after resize) or reuse the egui user texture that
    // wraps the world's colour target.
    let world_texture = match self.world_texture {
      Some(id) => {
        self.egui_renderer.update_egui_texture_from_wgpu_texture(
          &self.device,
          self.world.color_view(),
          wgpu::FilterMode::Linear,
          id,
        );
        id
      }
      None => {
        let id = self.egui_renderer.register_native_texture(
          &self.device,
          self.world.color_view(),
          wgpu::FilterMode::Linear,
        );
        self.world_texture = Some(id);
        id
      }
    };

    let raw_input = self.egui_state.take_egui_input(&self.window);
    self.egui_ctx.begin_pass(raw_input);

    let egui_ctx = self.egui_ctx.clone();
    let mut menu_actions: Vec<MenuAction> = Vec::new();
    let ctx = if defs_loaded {
      Some(Ctx::new(&*fs.structs, &*fs.mem))
    } else {
      None
    };

    // --- menu bar --------------------------------------------------------------
    //
    // egui 0.36 has no context-level `TopBottomPanel`, so the bar is
    // a top-anchored `Area` + `Frame::menu`. The render-config menus live on
    // `WorldRenderer::render_menu`; Attach + Tools are here.
    if defs_loaded {
      egui::Area::new(egui::Id::new("menu_bar"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(&egui_ctx, |ui| {
          egui::Frame::menu(ui.style()).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
              // Attach.
              let attached = fs.dolphin.get_attached_pid();
              let attach_title = if attached > 0 {
                format!("Attached ({attached})")
              } else {
                "Detatched".to_string()
              };
              ui.menu_button(attach_title, |ui| {
                ui.menu_button("Attach", |ui| {
                  if ui.button("Refresh").clicked() {
                    menu_actions.push(MenuAction::RefreshPids);
                  }
                  ui.separator();
                  for pid in fs.pids.iter() {
                    if ui.button(format!("{pid}")).clicked() {
                      menu_actions.push(MenuAction::Attach(pid.as_u32()));
                    }
                  }
                });
                if ui
                  .add_enabled(attached != 0, egui::Button::new("Detatch"))
                  .clicked()
                {
                  menu_actions.push(MenuAction::Detach);
                }
                if ui.button("Load from file").clicked() {
                  menu_actions.push(MenuAction::LoadFromFile);
                }
              });

              // Culling / Camera / Triggers / Actors.
              self.world.render_menu(ui);

              // Tools.
              ui.menu_button("Tools", |ui| {
                if ui.button("Reload Definitions").clicked() {
                  menu_actions.push(MenuAction::ReloadDefs);
                }
                ui.checkbox(fs.show_raw_data_view, "Raw Data View");
                ui.checkbox(
                  &mut fs.inspector.exact_values,
                  "Show exact floating point values",
                );
              });

              // FPS counter, pinned to the end of the toolbar.
              ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} FPS", self.fps_display));
              });
            });
          });
        });

      // "Camera Controls" window
      if self.world.show_exact_camera_controls {
        let mut open = true;
        egui::Window::new("Camera Controls")
          .resizable(false)
          .open(&mut open)
          .show(&egui_ctx, |ui| self.world.render_camera_controls(ui));
        if !open {
          self.world.show_exact_camera_controls = false;
        }
      }
    }

    // --- NOT LOADED error panel -------------------------------------
    //
    // Only shown while defs are missing — it's a blocking error with a Reload
    // action. The former loaded-state "Prime Watch" window was pure noise; the
    // "Loaded N structs" confirmation is a toast now (`App::new` / `ReloadDefs`).
    if !defs_loaded {
      egui::Window::new("NOT LOADED")
        .resizable(false)
        .collapsible(false)
        .show(&egui_ctx, |ui| {
          ui.label("Script definitions are not currently loaded.");
          ui.label("These are required to function.");
          ui.label("Current error:");
          ui.label(fs.status_text.as_str());
          if ui.button("Reload").clicked() {
            menu_actions.push(MenuAction::ReloadDefs);
          }
        });
    }

    // --- the offscreen 3D target + screen-space text overlays ---------------
    //
    // Drawn as a full-window background: an `Area` in egui's background layer
    // pinned to the screen rect, so every `Window`/`Area` floats above it and the
    // camera look/zoom drag is picked up on any part of the view not covered by
    // another window (no more fighting a monitored-object window for the pointer).
    let mut world_view_size_pts: Option<egui::Vec2> = None;
    let mut world_view_input = WorldViewInput::default();
    let screen_rect = egui_ctx.content_rect();
    egui::Area::new(egui::Id::new("world-background"))
      .order(egui::Order::Background)
      .fixed_pos(screen_rect.min)
      .show(&egui_ctx, |ui| {
        ui.set_min_size(screen_rect.size());
        let avail = screen_rect.size();
        world_view_size_pts = Some(avail);
        // Sense drag/scroll on the image itself — this is the camera look/zoom
        // input (see `WorldViewInput`), consumed next frame by `InputState::plan`.
        let resp = ui.add(
          egui::Image::new(egui::load::SizedTexture::new(world_texture, avail))
            .sense(egui::Sense::click_and_drag()),
        );
        let rect = resp.rect;
        if resp.dragged() {
          let d = resp.drag_delta();
          world_view_input.drag = (d.x, d.y);
        }
        if resp.hovered() {
          world_view_input.scroll = ui.input(|i| i.smooth_scroll_delta.y);
        }

        // Paint the queued overlays. `screen_pos` is in world-target physical pixels (Y-down,
        // already flipped by `getScreenspacePosFor*`); map it into the image
        // rect. Exact glyph centering is approximate — no shared font metrics.
        let (tw, th) = self.world_view_px;
        let sx = rect.width() / tw.max(1) as f32;
        let sy = rect.height() / th.max(1) as f32;
        let painter = ui.painter_at(rect);
        for ov in &self.world.text_overlays {
          let pos = rect.min + egui::vec2(ov.screen_pos.x * sx, ov.screen_pos.y * sy);
          painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            ov.text.as_str(),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
          );
        }
      });
    self.world_view_input = world_view_input;

    // --- globals inspector --------------- -------------------------
    if let Some(ctx) = ctx.as_ref() {
      egui::Window::new("globals").show(&egui_ctx, |ui| {
        egui::ScrollArea::vertical()
          .auto_shrink([false, true])
          .show(ui, |ui| {
            let sm = get_state_manager();
            fs.inspector.render(ui, ctx, "g_stateManager", &sm, true);
            let main = get_main();
            fs.inspector.render(ui, ctx, "g_main", &main, true);
            if let Some(mc) = get_memory_card(ctx) {
              fs.inspector.render(ui, ctx, "gp_MemoryCard", &mc, true);
            }
            if let Some(tp) = get_tweak_player(ctx) {
              fs.inspector.render(ui, ctx, "gp_TweakPlayer", &tp, true);
            }
          });
      });

      // --- Objects window + per-editor-ID watch windows --------------
      render_objects_window(
        &egui_ctx,
        ctx,
        &*fs.inspector,
        fs.objects,
        &mut *fs.editor_ids_to_watch,
        &mut *fs.show_active_in_table_only,
        &mut *fs.table_hovered_uid,
        &mut *fs.object_filter,
        &mut *fs.unknown_vtables,
      );
    }

    // --- Raw Data View --------------------------------------------
    if *fs.show_raw_data_view {
      let mut open = true;
      egui::Window::new("Raw view")
        .open(&mut open)
        .show(&egui_ctx, |ui| render_raw_data_view(ui, &fs.mem.data[..]));
      if !open {
        *fs.show_raw_data_view = false;
      }
    }

    // --- WorldStatus / PlayerStatus overlays, only while the memory parse is live.
    if let Some(ctx) = ctx.as_ref() {
      egui::Area::new(egui::Id::new("world-status-host"))
        .fixed_pos(egui::pos2(0.0, 24.0))
        .show(&egui_ctx, |ui| {
          self.world.render_status_windows(ctx, ui);
        });
    }

    // Ephemeral notifications, on top of everything else.
    fs.toasts.ui(&egui_ctx);

    let mut full_output = self.egui_ctx.end_pass();

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
    full_output.textures_delta.clear();

    self.queue.submit(
      user_cmd_bufs
        .into_iter()
        .chain(std::iter::once(encoder.finish())),
    );
    self.window.pre_present_notify();
    self.queue.present(frame);

    // Apply the menu actions collected during the egui pass
    for action in menu_actions {
      apply_menu_action(action, fs);
    }

    // Resize the offscreen target to match the "World" panel for the next frame
    // (documented one-frame lag).
    if let Some(sz) = world_view_size_pts {
      let ppp = full_output.pixels_per_point;
      let w = (sz.x * ppp).round().max(1.0) as u32;
      let h = (sz.y * ppp).round().max(1.0) as u32;
      self.world_view_px = (w, h);
      let _recreated = self.world.resize(&self.device, (w, h));
    }

    // Flush the UI layout to disk periodically so a crash doesn't lose it (the
    // authoritative save is in `App::exiting`).
    if self.last_ui_save.elapsed() >= ui_state::AUTOSAVE_INTERVAL {
      ui_state::save(&self.egui_ctx, &self.window);
      self.last_ui_save = Instant::now();
    }
  }
}
