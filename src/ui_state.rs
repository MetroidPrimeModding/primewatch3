//! Persist the egui UI layout across runs.
//!
//! Raw egui (we don't use eframe) keeps window positions/sizes and per-widget
//! state — `CollapsingHeader` open/closed, `ScrollArea` offsets — in
//! [`egui::Memory`], but never writes it anywhere. This module serializes that
//! struct to a RON file on shutdown and reinstalls it on startup, which is the
//! same mechanism eframe uses internally. The `persistence` feature on the
//! `egui` dependency is what makes `Memory` (de)serializable.

use std::path::PathBuf;
use std::time::Duration;

/// Minimum gap between the background saves `AppWindow::render` performs.
/// Matches eframe's default auto-save cadence — cheap insurance against losing
/// layout to a crash.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

/// Location of the serialized [`egui::Memory`]. Sits in the working directory
/// next to `./mem1.raw`, matching how the raw dump is auto-loaded in `App::new`.
fn state_path() -> PathBuf {
  PathBuf::from("./primewatch_ui.ron")
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
  match ron::from_str::<egui::Memory>(&text) {
    Ok(mem) => ctx.memory_mut(|m| *m = mem),
    Err(e) => eprintln!("ui_state: ignoring corrupt {}: {e}", path.display()),
  }
}

/// Serialize the current UI state to disk. Call on shutdown.
pub fn save(ctx: &egui::Context) {
  let mem = ctx.memory(|m| m.clone());
  let text = match ron::ser::to_string(&mem) {
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
