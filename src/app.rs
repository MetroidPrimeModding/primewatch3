//! Application shell: winit event loop + wgpu device/surface + the egui UI.
//!
//! Frame order: accumulate input -> per-frame memory parse ->
//! walk the live object list -> build the egui UI -> render the 3D world -> paint
//! egui. winit is event-driven, so input is accumulated from `WindowEvent`s and
//! consumed at the top of `RedrawRequested`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use sysinfo::Pid;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::ctx::Ctx;
use crate::inspector::Inspector;
use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::mem::game_object_utils::{TUniqueID, get_all_objects};
use crate::mem::globals::{get_main, get_memory_card, get_state_manager, get_tweak_player};
use crate::mem::vtables::vtable_class_name;
use crate::object_filter::ObjectFilter;
use crate::structs::prime_structs::{GameInstance, GameStructs};
use crate::toast::Toasts;
use crate::ui_state;
use crate::world::renderer::{CameraMode, WorldInput, WorldRenderer};

/// Build the event loop and run the app.
pub fn run() -> Result<(), Box<dyn Error>> {
  let event_loop = EventLoop::new()?;
  let mut app = App::new();
  event_loop.run_app(&mut app)?;

  // Hard-exit without running any destructors.
  //
  // egui-winit pulls in `smithay-clipboard`, which spawns a background thread
  // holding its own wayland `Connection` over the same `wl_display` winit owns.
  // Dropping `egui_winit::State` (inside `app`) tears down that clipboard state
  // concurrently with winit's own connection teardown, corrupting libwayland's
  // proxy object-map (`wl_map_insert_at` crash in `wl_proxy_destroy`). Letting
  // wgpu drop first doesn't help — the crash is in the clipboard drop itself.
  // `exiting()` already did the authoritative `ui_state::save`, so nothing on
  // this path needs Drop: kill the process and let the OS reclaim everything,
  // including the detached clipboard thread. `process::exit` diverges, so `app`
  // is simply never dropped.
  std::process::exit(0);
}

/// The five ghost-record hotkeys
const GHOST_KEYS: [KeyCode; 5] = [
  KeyCode::Digit1,
  KeyCode::Digit2,
  KeyCode::Digit3,
  KeyCode::Digit4,
  KeyCode::Digit5,
];

/// One entry in the "watch this editor ID" list.
/// Clicking a row in the "Objects" entity table upserts one of these; each
/// drives its own egui window and contributes its `last_known_uid` to the world
/// highlight set.
struct WatchedEditorId {
  eid: u32,
  last_known_uid: u16,
  type_name: String,
}

/// Raw winit input accumulated between frames, then folded into a [`WorldInput`]
/// (plus camera / ghost side effects) at the top of each frame.
#[derive(Default)]
struct InputState {
  keys_down: HashSet<KeyCode>,
  modifiers: ModifiersState,
}

/// One-frame-lagged interaction state of the "World" image widget, produced by
/// the egui pass and consumed by [`InputState::plan`] on the next frame (same
/// lag pattern as `world_view_px`).
#[derive(Default, Clone, Copy)]
struct WorldViewInput {
  /// Pointer drag delta over the image since the last frame, in egui points.
  drag: (f32, f32),
  /// Scroll delta while hovering the image, in egui points.
  scroll: f32,
}

/// Result of [`InputState::plan`] — a [`WorldInput`] plus the direct
/// `worldRenderer` mutations `processInput` performs (ghost record/clear,
/// detached-camera movement) and the resolved mouse-capture state.
struct InputPlan {
  world_input: WorldInput,
  ghost_record: [bool; 5],
  ghost_clear: [bool; 5],
  /// Net WASD/QE contribution for `CameraMode::Detached` (`forward = W - S`,
  /// `right = A - D`, `up = E - Q`).
  detached_move: (f32, f32, f32),
}

impl InputState {
  /// Folds accumulated input into an [`InputPlan`].
  /// Pure: the caller applies the plan.
  ///
  /// `world_view` carries last frame's drag/scroll over the "World" image
  /// (see [`WorldViewInput`]).
  fn plan(
    &self,
    wants_keyboard: bool,
    camera_mode: CameraMode,
    world_view: WorldViewInput,
  ) -> InputPlan {
    let mut wi = WorldInput::default();

    // Shift+N records ghost N, Ctrl+N clears it.
    let mut ghost_record = [false; 5];
    let mut ghost_clear = [false; 5];
    for (i, key) in GHOST_KEYS.iter().enumerate() {
      if self.keys_down.contains(key) {
        if self.modifiers.shift_key() {
          ghost_record[i] = true;
        } else if self.modifiers.control_key() {
          ghost_clear[i] = true;
        }
      }
    }

    // Mouse look + wheel zoom, driven by the "World" image's own drag/scroll
    // response (`world_view`). Yaw uses `yawSpeed = -0.005` (FPS-style:
    // drag right → look right). `scroll` is in egui points (~50/notch)
    wi.cam_pitch = world_view.drag.1 * 0.005;
    wi.cam_yaw = world_view.drag.0 * -0.005;
    wi.cam_zoom = world_view.scroll / 50.0 * -2.0;

    // Keyboard camera control.
    let mut detached_move = (0.0_f32, 0.0_f32, 0.0_f32);
    if !wants_keyboard {
      let down = |k| self.keys_down.contains(&k);
      if down(KeyCode::ArrowUp) {
        wi.cam_pitch += 0.03;
      }
      if down(KeyCode::ArrowDown) {
        wi.cam_pitch -= 0.03;
      }
      if down(KeyCode::ArrowLeft) {
        wi.cam_yaw -= 0.03;
      }
      if down(KeyCode::ArrowRight) {
        wi.cam_yaw += 0.03;
      }
      if down(KeyCode::PageUp) {
        wi.cam_zoom -= 0.5;
      }
      if down(KeyCode::PageDown) {
        wi.cam_zoom += 0.5;
      }
      if camera_mode == CameraMode::Detached {
        let axis = |a, b| i32::from(down(a)) as f32 - i32::from(down(b)) as f32;
        detached_move = (
          axis(KeyCode::KeyW, KeyCode::KeyS),
          axis(KeyCode::KeyA, KeyCode::KeyD),
          axis(KeyCode::KeyE, KeyCode::KeyQ),
        );
      }
    }

    InputPlan {
      world_input: wi,
      ghost_record,
      ghost_clear,
      detached_move,
    }
  }
}

/// Deferred menu action — collected during the egui pass (which only holds
/// shared borrows) and applied afterwards against the mutable game state.
enum MenuAction {
  RefreshPids,
  Attach(u32),
  Detach,
  LoadFromFile,
  ReloadDefs,
}

/// The mutable game/UI state `AppWindow::render` needs from the owning [`App`].
/// Handed in by reference so the wgpu/egui render state and the game state stay
/// separate fields (no `App`-owns-`AppWindow`-owns-`App` cycle).
struct FrameState<'a> {
  dolphin: &'a mut DolphinMemoryAccess,
  mem: &'a mut GameMemory,
  structs: &'a mut GameStructs,
  defs_loaded: &'a mut bool,
  status_text: &'a mut String,
  toasts: &'a mut Toasts,
  pids: &'a mut Vec<Pid>,
  show_raw_data_view: &'a mut bool,
  inspector: &'a mut Inspector,
  /// Live object list (walked in `redraw`, borrowed read-only here). Keyed by
  /// `TUniqueID`
  objects: &'a BTreeMap<TUniqueID, GameInstance>,
  /// Per-editor-ID watch windows.
  editor_ids_to_watch: &'a mut Vec<WatchedEditorId>,
  show_active_in_table_only: &'a mut bool,
  table_hovered_uid: &'a mut u16,
  object_filter: &'a mut ObjectFilter,
  /// Session-persistent set of unknown vtable addresses seen in the object list
  unknown_vtables: &'a mut BTreeSet<u32>,
}

/// Owns the long-lived game state plus the render state that only exists while
/// the window is active. No globals — everything is threaded explicitly.
struct App {
  /// Local MEM1 snapshot, refreshed each frame from `dolphin`.
  mem: GameMemory,
  dolphin: DolphinMemoryAccess,
  structs: GameStructs,
  /// Live object list, walked off `g_stateManager` once per frame
  objects: BTreeMap<TUniqueID, GameInstance>,
  defs_loaded: bool,
  /// Either "Loaded N structs and M enums" or the load error string.
  status_text: String,
  /// Cached Dolphin PID list for the Attach menu
  pids: Vec<Pid>,
  show_raw_data_view: bool,
  /// Generic `GameInstance` tree view — hosts the "globals" window and the
  /// Tools-menu exact-values toggle (`GameObjectRenderers::render_exact_values`).
  inspector: Inspector,
  editor_ids_to_watch: Vec<WatchedEditorId>,
  show_active_in_table_only: bool,
  table_hovered_uid: u16,
  object_filter: ObjectFilter,
  /// Session log of every unrecognised vtable address. Never shrinks.
  unknown_vtables: BTreeSet<u32>,
  /// Ephemeral corner notifications
  toasts: Toasts,
  /// Input accumulated between frames.
  input: InputState,
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

    // Offline dump path: auto-load
    // `./mem1.raw` when it sits next to the binary. A later live memcpy simply
    // overwrites it; a missing/short file is not fatal.
    if std::path::Path::new("./mem1.raw").exists() {
      match mem.load_from_file("./mem1.raw") {
        Ok(()) => println!("Loaded ./mem1.raw"),
        Err(err) => eprintln!("Failed to load ./mem1.raw: {err}"),
      }
    }

    // Auto-attach only when exactly one Dolphin is running
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

    let mut toasts = Toasts::default();
    if defs_loaded {
      toasts.info(&status_text);
    }

    Self {
      mem,
      dolphin,
      structs,
      objects: BTreeMap::new(),
      defs_loaded,
      status_text,
      pids,
      toasts,
      show_raw_data_view: false,
      inspector: Inspector::new(),
      editor_ids_to_watch: Vec::new(),
      show_active_in_table_only: true,
      table_hovered_uid: 0xFFFF,
      object_filter: ObjectFilter::default(),
      unknown_vtables: BTreeSet::new(),
      input: InputState::default(),
      window: None,
    }
  }

  /// One `RedrawRequested`: consume accumulated input, refresh memory, walk the
  /// object list, update + render the world, paint egui
  fn redraw(&mut self) {
    let App {
      window,
      mem,
      dolphin,
      structs,
      objects,
      defs_loaded,
      status_text,
      toasts,
      pids,
      show_raw_data_view,
      inspector,
      editor_ids_to_watch,
      show_active_in_table_only,
      table_hovered_uid,
      object_filter,
      unknown_vtables,
      input,
    } = self;
    let Some(window) = window.as_mut() else {
      return;
    };

    if *defs_loaded {
      // Refresh the snapshot (no-op while detached).
      mem.update_from_dolphin(dolphin);

      // Consume accumulated input into a plan, then apply it (ghost record/clear,
      // detached-camera move). Camera look/zoom comes from last frame's
      // drag/scroll over the "World" image (`world_view_input`).
      let wants_kb = window.egui_ctx.egui_wants_keyboard_input();
      let plan = input.plan(wants_kb, window.world.camera_mode, window.world_view_input);

      for (i, &rec) in plan.ghost_record.iter().enumerate() {
        if rec {
          window.world.record_player_ghost(i);
        }
      }
      for (i, &clr) in plan.ghost_clear.iter().enumerate() {
        if clr {
          window.world.clear_player_ghost(i);
        }
      }
      let (mf, mr, mu) = plan.detached_move;
      if mf != 0.0 || mr != 0.0 || mu != 0.0 {
        window.world.move_detached_camera(mf, mr, mu);
      }

      // Walk the live object list, then update the world.
      let ctx = Ctx::new(structs, mem);
      *objects = get_all_objects(&ctx);
      let viewport = window.world_view_px;
      // The world highlight set: the uid the "Objects"
      // table row cursor is over, plus every watched editor ID's last-known uid.
      let mut highlighted: HashSet<u16> = HashSet::new();
      if *table_hovered_uid != 0xFFFF {
        highlighted.insert(*table_hovered_uid);
      }
      for watch in editor_ids_to_watch.iter() {
        highlighted.insert(watch.last_known_uid);
      }
      window
        .world
        .update(&ctx, &plan.world_input, viewport, objects, &highlighted);
    }

    // `objects` is walked above and consumed (by `&`) by `world.update`
    let mut fs = FrameState {
      dolphin,
      mem,
      structs,
      defs_loaded,
      status_text,
      toasts,
      pids,
      show_raw_data_view,
      inspector,
      objects: &*objects,
      editor_ids_to_watch,
      show_active_in_table_only,
      table_hovered_uid,
      object_filter,
      unknown_vtables,
    };
    window.render(&mut fs);
  }

  /// Accumulate one `WindowEvent` into [`InputState`] (called after egui has had
  /// its chance to claim the event).
  fn accumulate_input(&mut self, event: &WindowEvent) {
    match event {
      WindowEvent::KeyboardInput { event, .. } => {
        if let PhysicalKey::Code(code) = event.physical_key {
          match event.state {
            ElementState::Pressed => {
              self.input.keys_down.insert(code);
            }
            ElementState::Released => {
              self.input.keys_down.remove(&code);
            }
          }
        }
      }
      WindowEvent::ModifiersChanged(m) => self.input.modifiers = m.state(),
      WindowEvent::Focused(false) => {
        // Drop held keys so nothing sticks while unfocused.
        self.input.keys_down.clear();
      }
      _ => {}
    }
  }
}

/// A minimal read-only hex dump over the MEM1 snapshot — a small custom table
/// rather than adding `egui_memory_editor`. Offsets are raw snapshot offsets (base 0)
fn render_raw_data_view(ui: &mut egui::Ui, data: &[u8]) {
  const BYTES_PER_ROW: usize = 16;
  let rows = data.len().div_ceil(BYTES_PER_ROW);
  let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
  egui::ScrollArea::vertical()
    .auto_shrink([false, false])
    .show_rows(ui, row_h, rows, |ui, range| {
      for row in range {
        let start = row * BYTES_PER_ROW;
        let end = (start + BYTES_PER_ROW).min(data.len());
        let chunk = &data[start..end];
        let mut line = format!("{start:08x}  ");
        for b in chunk {
          line.push_str(&format!("{b:02x} "));
        }
        for _ in chunk.len()..BYTES_PER_ROW {
          line.push_str("   ");
        }
        line.push(' ');
        for &b in chunk {
          line.push(if (0x20..0x7f).contains(&b) {
            b as char
          } else {
            '.'
          });
        }
        ui.add(
          egui::Label::new(egui::RichText::new(line).monospace())
            .wrap_mode(egui::TextWrapMode::Extend),
        );
      }
    });
}

/// The "Objects" window (count, vtable aggregation, "List of types" table,
/// filter + entity table) plus the per-`WatchedEditorId` watch-window loop
///
/// All state mutated here is local UI state (no memory writes), so it mutates
/// the passed `&mut` refs directly rather than deferring like `MenuAction`.
#[allow(clippy::too_many_arguments)]
fn render_objects_window(
  egui_ctx: &egui::Context,
  ctx: &Ctx,
  inspector: &Inspector,
  objects: &BTreeMap<TUniqueID, GameInstance>,
  editor_ids_to_watch: &mut Vec<WatchedEditorId>,
  show_active_in_table_only: &mut bool,
  table_hovered_uid: &mut u16,
  object_filter: &mut ObjectFilter,
  unknown_vtables: &mut BTreeSet<u32>,
) {
  // Build the lookup maps and vtable histogram from the live object list.
  let mut vtables: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
  let mut eid_to_entity: HashMap<u32, &GameInstance> = HashMap::new();
  let mut uid_to_entity: HashMap<u16, &GameInstance> = HashMap::new();
  for entity in objects.values() {
    let vtable = entity.member(ctx, "vtable").read_u32(ctx).unwrap_or(0);
    let active = entity.member(ctx, "active").read_bool(ctx).unwrap_or(false);
    let slot = vtables.entry(vtable).or_insert((0, 0));
    if active {
      slot.0 += 1;
    } else {
      slot.1 += 1;
    }
    let eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
    let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
    eid_to_entity.insert(eid, entity);
    uid_to_entity.insert(uid, entity);
  }

  // Accumulate never-before-seen vtable addresses. The
  // `> 0x80000000 && < 0x80700000` window skips the "not up to date yet"
  // sub-0x80000000 garbage.
  for &vtable in vtables.keys() {
    if vtable_class_name(vtable).is_none() && vtable > 0x8000_0000 && vtable < 0x8070_0000 {
      unknown_vtables.insert(vtable);
    }
  }

  // `objects` is now a `BTreeMap`, so `iter()` is already sorted by `TUniqueID`.
  let ordered: Vec<(&TUniqueID, &GameInstance)> = objects.iter().collect();

  egui::Window::new("Objects").show(egui_ctx, |ui| {
    ui.label(format!("Current object count: {}", objects.len()));

    // "Copy unknowns (N)".
    if ui
      .button(format!("Copy unknowns ({})", unknown_vtables.len()))
      .clicked()
    {
      let mut clip = String::new();
      for vt in unknown_vtables.iter() {
        clip.push_str(&format!("{{0x{vt:08x}, \"\"}},\n"));
      }
      ui.ctx().copy_text(clip);
    }

    // "List of types" 4-col table.
    egui::CollapsingHeader::new("List of types").show(ui, |ui| {
      egui::Grid::new("objects_vtables")
        .striped(true)
        .show(ui, |ui| {
          ui.label("address");
          ui.label("name");
          ui.label("active");
          ui.label("inactive");
          ui.end_row();
          for (&vtable, &(active, inactive)) in &vtables {
            if ui
              .selectable_label(false, format!("{vtable:08x}"))
              .clicked()
            {
              ui.ctx().copy_text(format!("{{0x{vtable:08x}, \"\"}},"));
            }
            ui.label(vtable_class_name(vtable).unwrap_or("unknown"));
            ui.label(active.to_string());
            ui.label(inactive.to_string());
            ui.end_row();
          }
        });
    });

    // Filter hint, filter box, "show active only".
    ui.label("Editor ID: #38 Class: @CPlayer name: &name");
    ui.label("(or just type what you're looking for)");
    object_filter.ui(ui);
    ui.checkbox(show_active_in_table_only, "Show active only");

    // Reset before the table; row hover sets it.
    *table_hovered_uid = 0xFFFF;

    // The 5-col scrolling entity table.
    egui::ScrollArea::vertical()
      .max_height(400.0)
      .auto_shrink([false, false])
      .show(ui, |ui| {
        egui::Grid::new("objects_entities")
          .striped(true)
          .show(ui, |ui| {
            ui.label("class");
            ui.label("editorID");
            ui.label("uniqueID");
            ui.label("active");
            ui.label("name");
            ui.end_row();

            for (_, entity) in &ordered {
              let active = entity.member(ctx, "active").read_bool(ctx).unwrap_or(false);
              if *show_active_in_table_only && !active {
                continue;
              }
              let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
              let eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
              let name = entity
                .member(ctx, "name")
                .read_string(ctx)
                .unwrap_or_default();

              // Probe string; sigils `#`/`@`/`&` let a user filter by editor ID
              // / class / name. First `{:08x}` is hex eid, second `{:08}` is
              // decimal eid zero-padded.
              let probe = format!("#{eid:08x}#{eid:08}@{}&{}", entity.type_name, name);
              if !object_filter.passes(&probe) {
                continue;
              }

              let resp = ui.selectable_label(false, entity.type_name.as_str());
              if resp.clicked() {
                if let Some(watch) = editor_ids_to_watch.iter_mut().find(|w| w.eid == eid) {
                  watch.last_known_uid = uid;
                  watch.type_name = entity.type_name.clone();
                } else {
                  editor_ids_to_watch.push(WatchedEditorId {
                    eid,
                    last_known_uid: uid,
                    type_name: entity.type_name.clone(),
                  });
                }
              }
              if resp.hovered() {
                *table_hovered_uid = uid;
              }

              ui.label(format!("{eid:08x}"));
              ui.label(format!("{uid:04x}"));
              ui.label(if active { "yes" } else { "no" });
              ui.label(name);
              ui.end_row();
            }
          });
      });

    ui.label(format!("tableHoveredUid: {}", *table_hovered_uid));
  });

  // One window per watched editor ID. Index-based loop so a window closing
  // (removing its entry) can't skip or panic on the next.
  let mut i = 0;
  while i < editor_ids_to_watch.len() {
    let (eid, last_known_uid, type_name) = {
      let w = &editor_ids_to_watch[i];
      (w.eid, w.last_known_uid, w.type_name.clone())
    };
    let title = format!("{type_name} {eid:08x}");
    let mut open = true;
    let mut new_last_known: Option<u16> = None;

    egui::Window::new(&title)
      .open(&mut open)
      .id(egui::Id::new(("watch", eid)))
      .min_size([240.0, 200.0])
      .show(egui_ctx, |ui| {
        egui::ScrollArea::vertical()
          .auto_shrink([false, true])
          .show(ui, |ui| {
            // Resolve by last-known uid, then by editor ID, then give up.
            let mut handled = false;
            if let Some(entity) = uid_to_entity.get(&last_known_uid) {
              let e_eid = entity.member(ctx, "editorID").read_u32(ctx).unwrap_or(0);
              if e_eid == eid && entity.type_name == type_name {
                inspector.render(ui, ctx, &type_name, entity, false);
                handled = true;
              }
            }
            if !handled && let Some(entity) = eid_to_entity.get(&eid) {
              let uid = entity.member(ctx, "uniqueID").read_u16(ctx).unwrap_or(0);
              new_last_known = Some(uid);
              inspector.render(ui, ctx, &type_name, entity, false);
              handled = true;
            }
            if !handled {
              ui.label("Not loaded");
            }
          });
      });

    if let Some(uid) = new_last_known {
      editor_ids_to_watch[i].last_known_uid = uid;
    }
    if open {
      i += 1;
    } else {
      editor_ids_to_watch.remove(i);
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
    // Route to egui first so it can claim the event
    if let Some(window) = self.window.as_mut() {
      let _ = window.egui_state.on_window_event(&window.window, &event);
    } else {
      return;
    }

    self.accumulate_input(&event);

    match event {
      WindowEvent::CloseRequested => event_loop.exit(),
      WindowEvent::Resized(size) => {
        if let Some(window) = self.window.as_mut() {
          window.resize(size);
        }
      }
      WindowEvent::ScaleFactorChanged { .. } => {
        if let Some(window) = self.window.as_ref() {
          window.window.request_redraw();
        }
      }
      WindowEvent::RedrawRequested => self.redraw(),
      _ => {}
    }
  }

  fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
    // TODO: only redraw on demand / frame-pace instead of spinning.
    if let Some(window) = self.window.as_ref() {
      window.window.request_redraw();
    }
  }

  fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
    // Authoritative UI-layout save: covers window-close, menu quit, and any
    // other clean exit (`render` also autosaves for the crash case).
    if let Some(window) = self.window.as_ref() {
      ui_state::save(&window.egui_ctx);
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
  world: WorldRenderer,
  /// egui user-texture id for `world`'s colour target. `None` until the first
  /// `register_native_texture`; reused via `update_egui_texture_from_wgpu_texture`
  /// thereafter (that call rebuilds the bind group, so it survives target resize).
  world_texture: Option<egui::TextureId>,
  /// Last frame's "World" panel size in physical pixels — fed to
  /// `WorldRenderer::update` this frame (documented one-frame lag). Seeded with
  /// the initial target size.
  world_view_px: (u32, u32),
  /// Last frame's drag/scroll over the "World" image — fed to [`InputState::plan`]
  /// this frame (same one-frame lag as `world_view_px`).
  world_view_input: WorldViewInput,
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
  fn new(event_loop: &ActiveEventLoop) -> Result<Self, Box<dyn Error>> {
    let window = Arc::new(
      event_loop.create_window(
        Window::default_attributes()
          .with_title("Prime Watch 3")
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

    let world = WorldRenderer::new(&device, (800, 600));

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
  fn resize(&mut self, size: PhysicalSize<u32>) {
    if size.width > 0 && size.height > 0 {
      self.config.width = size.width;
      self.config.height = size.height;
      self.surface.configure(&self.device, &self.config);
    }
    self.window.request_redraw();
  }

  /// One frame: build the egui UI, render the 3D world, clear to black, paint
  /// egui. `fs` carries the game/UI state owned by [`App`].
  fn render(&mut self, fs: &mut FrameState) {
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
              ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                  ui.label(format!("{} FPS", self.fps_display));
                },
              );
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
      ui_state::save(&self.egui_ctx);
      self.last_ui_save = Instant::now();
    }
  }
}

/// Apply one deferred [`MenuAction`] against the mutable game state.
fn apply_menu_action(action: MenuAction, fs: &mut FrameState) {
  match action {
    MenuAction::RefreshPids => {
      *fs.pids = fs.dolphin.get_dolphin_pids();
    }
    MenuAction::Attach(pid) => {
      if fs.dolphin.attach_to_process(pid as i32) {
        println!("Attached to Dolphin pid {pid}");
      } else {
        eprintln!("Failed to attach to Dolphin pid {pid}");
      }
    }
    MenuAction::Detach => fs.dolphin.detach_from_process(),
    MenuAction::LoadFromFile => {
      // `rfd` native picker; detach before loading a dump.
      if let Some(path) = rfd::FileDialog::new()
        .add_filter("Memory dump", &["raw"])
        .set_directory(".")
        .pick_file()
      {
        fs.dolphin.detach_from_process();
        match fs.mem.load_from_file(&path.to_string_lossy()) {
          Ok(()) => println!("Loaded {}", path.display()),
          Err(err) => eprintln!("Failed to load {}: {err}", path.display()),
        }
      }
    }
    MenuAction::ReloadDefs => {
      // Fresh registry so removed `.bs` entries don't linger
      *fs.structs = GameStructs::new_empty();
      match fs.structs.load_from_dir("prime_defs") {
        Ok(()) => {
          *fs.status_text = format!(
            "Loaded {} structs and {} enums",
            fs.structs.structs.len(),
            fs.structs.enums.len()
          );
          *fs.defs_loaded = true;
          println!("{}", fs.status_text);
          fs.toasts.info(fs.status_text.as_str());
        }
        Err(err) => {
          *fs.defs_loaded = false;
          eprintln!("Error loading structs: {err}");
          fs.toasts
            .error(format!("Failed to load definitions: {err}"));
          *fs.status_text = err;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn state() -> InputState {
    InputState::default()
  }

  #[test]
  fn world_drag_drives_yaw_pitch() {
    let s = state();
    let wv = WorldViewInput {
      drag: (10.0, 4.0),
      scroll: 0.0,
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv);
    // Rightward drag → negative yaw (FPS-style: look right).
    assert!((plan.world_input.cam_yaw - (10.0 * -0.005)).abs() < 1e-6);
    assert!((plan.world_input.cam_pitch - (4.0 * 0.005)).abs() < 1e-6);
  }

  #[test]
  fn world_scroll_drives_zoom() {
    let s = state();
    let wv = WorldViewInput {
      drag: (0.0, 0.0),
      scroll: 50.0,
    };
    let plan = s.plan(false, CameraMode::FollowPlayer, wv);
    assert!((plan.world_input.cam_zoom - (50.0 / 50.0 * -2.0)).abs() < 1e-6);
  }

  #[test]
  fn no_world_view_input_means_no_camera_motion() {
    let s = state();
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.world_input.cam_yaw, 0.0);
    assert_eq!(plan.world_input.cam_pitch, 0.0);
    assert_eq!(plan.world_input.cam_zoom, 0.0);
  }

  #[test]
  fn shift_and_ctrl_digits_record_and_clear_ghosts() {
    let mut s = state();
    s.keys_down.insert(KeyCode::Digit3);
    s.modifiers = ModifiersState::SHIFT;
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.ghost_record, [false, false, true, false, false]);
    assert_eq!(plan.ghost_clear, [false; 5]);

    s.modifiers = ModifiersState::CONTROL;
    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.ghost_clear, [false, false, true, false, false]);
    assert_eq!(plan.ghost_record, [false; 5]);
  }

  #[test]
  fn wasd_only_moves_in_detached_mode_and_arrows_always_work() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowLeft);

    let plan = s.plan(false, CameraMode::FollowPlayer, WorldViewInput::default());
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert!((plan.world_input.cam_yaw - -0.03).abs() < 1e-6);

    let plan = s.plan(false, CameraMode::Detached, WorldViewInput::default());
    assert_eq!(plan.detached_move, (1.0, 0.0, 0.0));
  }

  #[test]
  fn keyboard_capture_blocks_movement_keys() {
    let mut s = state();
    s.keys_down.insert(KeyCode::KeyW);
    s.keys_down.insert(KeyCode::ArrowUp);
    let plan = s.plan(true, CameraMode::Detached, WorldViewInput::default());
    assert_eq!(plan.detached_move, (0.0, 0.0, 0.0));
    assert_eq!(plan.world_input.cam_pitch, 0.0);
  }
}
