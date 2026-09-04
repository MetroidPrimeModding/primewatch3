//! Persist the egui UI layout, and the OS window's size/position, across runs.
//!
//! Raw egui (we don't use eframe) keeps window positions/sizes and per-widget
//! state — `CollapsingHeader` open/closed, `ScrollArea` offsets — in
//! [`egui::Memory`], but never writes it anywhere. This module serializes that
//! struct to a RON file on shutdown and reinstalls it on startup, which is the
//! same mechanism eframe uses internally. The `persistence` feature on the
//! `egui` dependency is what makes `Memory` (de)serializable.
//!
//! `egui::Memory`'s window/area positions are only meaningful relative to the
//! OS window's own size — egui constrains them to stay inside the visible
//! screen rect. So the OS window's outer position and inner size are saved
//! here too; without that, the app always reopened at a fixed 1200x800, and a
//! window dragged near the edge of a larger viewport would get clamped back
//! inward on the next launch (looking like it "moved" on its own).

use std::path::PathBuf;
use std::time::Duration;

use winit::window::Window;

/// Minimum gap between the background saves `AppWindow::render` performs.
/// Matches eframe's default auto-save cadence — cheap insurance against losing
/// layout to a crash.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

/// Location of the serialized UI state. Sits in the working directory next to
/// `./mem1.raw`, matching how the raw dump is auto-loaded in `App::new`.
fn state_path() -> PathBuf {
  PathBuf::from("./primewatch_ui.ron")
}

/// OS window outer position and inner (logical) size, saved alongside
/// `egui::Memory` so the egui viewport a layout was saved against can be
/// recreated before that layout is reinstalled.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct WindowGeometry {
  /// `None` on platforms that don't report/support an outer position (e.g.
  /// Wayland) — the window is created but left wherever the compositor puts it.
  pub position: Option<(f64, f64)>,
  pub size: (f64, f64),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedState {
  memory: egui::Memory,
  window: Option<WindowGeometry>,
}

/// Read the saved window geometry, if any, without touching `ctx`. Call
/// before creating the window so it can be created at the right size/position
/// instead of `AppWindow`'s default. A missing or corrupt file yields `None`
/// (first run, or a manually-deleted/edited file); errors are swallowed here —
/// `load` reports them once the same file is re-read.
pub fn load_window_geometry() -> Option<WindowGeometry> {
  let text = std::fs::read_to_string(state_path()).ok()?;
  ron::from_str::<PersistedState>(&text).ok()?.window
}

/// Reinstall persisted UI state into `ctx`. Call once, right after the
/// [`egui::Context`] is created and before the first frame. A missing file is
/// normal (first run); a corrupt one is logged and ignored.
pub fn load(ctx: &egui::Context) {
  let path = state_path();
  let text = match std::fs::read_to_string(&path) {
    Ok(t) => t,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
    Err(e) => {
      eprintln!("ui_state: could not read {}: {e}", path.display());
      return;
    }
  };
  match ron::from_str::<PersistedState>(&text) {
    Ok(state) => ctx.memory_mut(|m| *m = state.memory),
    Err(e) => eprintln!("ui_state: ignoring corrupt {}: {e}", path.display()),
  }
}

/// Serialize the current UI state, plus `window`'s outer position/inner size,
/// to disk. Call on shutdown (and periodically as a crash-insurance autosave).
pub fn save(ctx: &egui::Context, window: &Window) {
  let memory = ctx.memory(|m| m.clone());
  let size = window.inner_size().to_logical(window.scale_factor());
  let position = window
    .outer_position()
    .ok()
    .map(|p| p.to_logical(window.scale_factor()))
    .map(|p: winit::dpi::LogicalPosition<f64>| (p.x, p.y));
  let state = PersistedState {
    memory,
    window: Some(WindowGeometry {
      position,
      size: (size.width, size.height),
    }),
  };
  let text = match ron::ser::to_string(&state) {
    Ok(t) => t,
    Err(e) => {
      eprintln!("ui_state: serialize failed: {e}");
      return;
    }
  };
  let path = state_path();
  if let Err(e) = std::fs::write(&path, &text) {
    eprintln!("ui_state: could not write {}: {e}", path.display());
  }
}
