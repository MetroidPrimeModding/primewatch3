//! Deferred menu actions, collected during the egui pass (which only holds
//! shared borrows) and applied afterwards against the mutable game state.

use crate::structs::prime_structs::GameStructs;

use super::FrameState;

/// Deferred menu action — collected during the egui pass (which only holds
/// shared borrows) and applied afterwards against the mutable game state.
pub(super) enum MenuAction {
  RefreshPids,
  Attach(u32),
  Detach,
  LoadFromFile,
  ReloadDefs,
}

/// Apply one deferred [`MenuAction`] against the mutable game state.
pub(super) fn apply_menu_action(action: MenuAction, fs: &mut FrameState) {
  match action {
    MenuAction::RefreshPids => {
      *fs.pids = fs.dolphin.get_dolphin_pids();
    }
    MenuAction::Attach(pid) => {
      *fs.awaiting_dolphin_reconnect = false;
      if fs.dolphin.attach_to_process(pid as i32) {
        println!("Attached to Dolphin pid {pid}");
      } else {
        eprintln!("Failed to attach to Dolphin pid {pid}");
      }
    }
    MenuAction::Detach => {
      *fs.awaiting_dolphin_reconnect = false;
      fs.dolphin.detach_from_process();
    }
    MenuAction::LoadFromFile => {
      // `rfd` native picker; detach before loading a dump.
      if let Some(path) = rfd::FileDialog::new()
        .add_filter("Memory dump", &["raw"])
        .set_directory(".")
        .pick_file()
      {
        *fs.awaiting_dolphin_reconnect = false;
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
