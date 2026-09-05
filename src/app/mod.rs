//! Application shell: winit event loop + wgpu device/surface + the egui UI.
//!
//! Frame order: accumulate input -> per-frame memory parse ->
//! walk the live object list -> build the egui UI -> render the 3D world -> paint
//! egui. winit is event-driven, so input is accumulated from `WindowEvent`s and
//! consumed at the top of `RedrawRequested`.

mod app_window;
mod input;
mod menu_action;
mod objects_window;
mod raw_data_view;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::time::{Duration, Instant};

use sysinfo::Pid;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

use crate::ctx::Ctx;
use crate::inspector::Inspector;
use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::mem::game_object_utils::{TUniqueID, get_all_objects};
use crate::object_filter::ObjectFilter;
use crate::structs::prime_structs::{GameInstance, GameStructs};
use crate::toast::Toasts;
use crate::ui_state;

use app_window::AppWindow;
use input::InputState;
use objects_window::WatchedEditorId;

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
  /// Cleared on any explicit attach/detach/load-from-file so a manual detach
  /// doesn't trigger the auto-reconnect scan meant for a natural disconnect.
  awaiting_dolphin_reconnect: &'a mut bool,
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
  /// Set when Dolphin disconnects on its own (process exited) rather than via
  /// an explicit Detach/Load-from-file/Attach action. While set, `redraw`
  /// rescans for a Dolphin process every `DOLPHIN_POLL_INTERVAL` and
  /// auto-attaches if exactly one is found, mirroring the startup auto-attach.
  awaiting_dolphin_reconnect: bool,
  /// Throttles both the attached-process liveness check and the reconnect
  /// scan to once per `DOLPHIN_POLL_INTERVAL`, independent of frame rate.
  last_dolphin_poll: Instant,
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
      awaiting_dolphin_reconnect: true,
      last_dolphin_poll: Instant::now(),
      input: InputState::default(),
      window: None,
    }
  }

  /// Once per `DOLPHIN_POLL_INTERVAL`: notice a Dolphin process that exited on
  /// its own and, while awaiting reconnect, rescan for a single Dolphin
  /// instance to auto-attach to (same rule as the startup auto-attach).
  const DOLPHIN_POLL_INTERVAL: Duration = Duration::from_secs(5);

  fn poll_dolphin_connection(&mut self) {
    if self.last_dolphin_poll.elapsed() < Self::DOLPHIN_POLL_INTERVAL {
      return;
    }
    self.last_dolphin_poll = Instant::now();

    if self.dolphin.get_attached_pid() > 0 {
      if !self.dolphin.is_attached_process_alive() {
        println!("Dolphin process exited; scanning for a new instance");
        self.dolphin.detach_from_process();
        self
          .toasts
          .info("Dolphin disconnected; scanning for a new instance...");
        self.awaiting_dolphin_reconnect = true;
      }
      return;
    }

    if !self.awaiting_dolphin_reconnect {
      return;
    }

    self.pids = self.dolphin.get_dolphin_pids();
    if self.pids.len() == 1 {
      let pid = self.pids[0].as_u32() as i32;
      if self.dolphin.attach_to_process(pid) {
        println!("Reattached to Dolphin pid {pid}");
        self.toasts.info(format!("Reattached to Dolphin pid {pid}"));
        self.awaiting_dolphin_reconnect = false;
      } else {
        eprintln!("Failed to reattach to Dolphin pid {pid}");
      }
    }
  }

  /// One `RedrawRequested`: consume accumulated input, refresh memory, walk the
  /// object list, update + render the world, paint egui
  fn redraw(&mut self) {
    self.poll_dolphin_connection();

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
      awaiting_dolphin_reconnect,
      input,
      last_dolphin_poll: _,
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
      awaiting_dolphin_reconnect,
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
      ui_state::save(&window.egui_ctx, &window.window);
    }
  }
}
