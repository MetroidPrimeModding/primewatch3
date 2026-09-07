//! Script discovery, per-frame execution, and enabled-state persistence.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use rhai::{AST, Engine};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::TUniqueID;
use crate::structs::prime_structs::GameInstance;

use super::engine::build_engine;
use super::env::ScriptFrame;
use super::window::CustomInspectorWindow;

pub(super) const SCRIPT_DIR: &str = "scripts";

const ENABLED_STATE_PATH: &str = "./primewatch_scripts.ron";

fn load_enabled_state() -> BTreeMap<String, bool> {
  std::fs::read_to_string(ENABLED_STATE_PATH)
    .ok()
    .and_then(|text| ron::from_str::<BTreeMap<String, bool>>(&text).ok())
    .unwrap_or_default()
}

pub struct LoadedScript {
  pub name: String,
  pub path: PathBuf,
  pub compiled: Result<AST, String>,
  pub enabled: bool,
  pub runtime_error: Option<String>,
}

pub struct ScriptManager {
  engine: Engine,
  dir: PathBuf,
  pub entries: Vec<LoadedScript>,
  pub scan_error: Option<String>,
  pub show_window: bool,
}

impl ScriptManager {
  pub fn new() -> Self {
    let mut manager = ScriptManager {
      engine: build_engine(),
      dir: PathBuf::from(SCRIPT_DIR),
      entries: Vec::new(),
      scan_error: None,
      show_window: false,
    };
    manager.reload();
    manager
  }

  pub fn dir_display(&self) -> String {
    self.dir.display().to_string()
  }

  pub fn reload(&mut self) {
    let previously_enabled: HashMap<String, bool> = self
      .entries
      .iter()
      .map(|s| (s.name.clone(), s.enabled))
      .collect();
    self.entries.clear();
    self.scan_error = None;

    let persisted_enabled = load_enabled_state();

    let read_dir = match std::fs::read_dir(&self.dir) {
      Ok(rd) => rd,
      Err(err) => {
        if err.kind() != std::io::ErrorKind::NotFound {
          self.scan_error = Some(format!("{}: {err}", self.dir.display()));
        }
        return;
      }
    };

    let mut paths: Vec<PathBuf> = read_dir
      .filter_map(|e| e.ok().map(|e| e.path()))
      .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rhai"))
      .collect();
    paths.sort();

    for path in paths {
      let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
      let enabled = previously_enabled
        .get(&name)
        .or_else(|| persisted_enabled.get(&name))
        .copied()
        .unwrap_or(true);
      let compiled = std::fs::read_to_string(&path)
        .map_err(|err| format!("read error: {err}"))
        .and_then(|src| self.engine.compile(&src).map_err(|err| err.to_string()));
      self.entries.push(LoadedScript {
        name,
        path,
        compiled,
        enabled,
        runtime_error: None,
      });
    }
  }

  pub fn persist_enabled(&self) {
    let map: BTreeMap<&str, bool> = self
      .entries
      .iter()
      .map(|s| (s.name.as_str(), s.enabled))
      .collect();
    match ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default()) {
      Ok(text) => {
        if let Err(err) = std::fs::write(ENABLED_STATE_PATH, text) {
          eprintln!("scripting: could not write {ENABLED_STATE_PATH}: {err}");
        }
      }
      Err(err) => eprintln!("scripting: could not serialize enabled state: {err}"),
    }
  }

  pub fn run_frame(
    &mut self,
    ctx: &Ctx,
    objects: &BTreeMap<TUniqueID, GameInstance>,
  ) -> Vec<CustomInspectorWindow> {
    let ScriptManager {
      engine, entries, ..
    } = self;
    let frame = ScriptFrame::enter(ctx, objects);
    let mut windows = Vec::new();
    for script in entries.iter_mut() {
      let (true, Ok(ast)) = (script.enabled, &script.compiled) else {
        continue;
      };
      match engine.run_ast(ast) {
        Ok(()) => script.runtime_error = None,
        Err(err) => script.runtime_error = Some(err.to_string()),
      }
      windows.append(&mut frame.take_windows());
    }
    windows
  }
}

impl Default for ScriptManager {
  fn default() -> Self {
    Self::new()
  }
}
