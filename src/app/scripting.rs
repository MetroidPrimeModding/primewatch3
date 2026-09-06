//! Rhai scripting bridge.
//!
//! Scripts live as `*.rhai` files in `./scripts/` and are **re-run every frame**
//! from [`super::App::redraw`], right after the per-frame memory parse and object
//! walk. A script inspects live memory through a small handle API and hands back
//! [`CustomInspectorWindow`]s, which the app draws with the normal [`Inspector`].
//!
//! ## The `Ctx` problem
//!
//! Rhai can only register functions whose arguments are `'static` values (plus an
//! optional leading `NativeCallContext`). Our traversal layer needs a
//! `&Ctx<'a>` (borrows `GameStructs` + `GameMemory`), which is neither `'static`
//! nor a value. Registering `fn(&mut GameInstance, &Ctx)` *compiles* but rhai can
//! never supply the second argument, so the function is silently uncallable.
//!
//! So the bridge threads `Ctx` (and the pre-walked object map) through a
//! thread-local set for exactly the duration of one script run by [`ScriptFrame`].
//! Every registered function is a capture-free closure that pulls them back out
//! via [`with_env`]. This is sound because scripts run **synchronously on one
//! thread** and never retain a handle past the call: the `ScriptFrame` guard
//! borrows both referents and clears the thread-local on drop. It is a transient
//! call-scoped bridge, not the ambient mutable game state the port set out to
//! remove.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::path::PathBuf;

use rhai::{AST, Dynamic, Engine, EvalAltResult, Position};

use crate::ctx::Ctx;
use crate::mem::game_object_utils::TUniqueID;
use crate::mem::globals::{get_main, get_state_manager};
use crate::structs::prime_structs::GameInstance;

#[derive(Clone)]
pub enum CustomInspectorRow {
  /// A live handle: re-resolved and re-read from memory by the inspector every
  /// frame. `label` is the tree-node caption.
  Instance {
    label: String,
    instance: GameInstance,
  },
  Text(String),
}

#[derive(Clone)]
pub struct CustomInspectorWindow {
  pub title: String,
  pub rows: Vec<CustomInspectorRow>,
}

impl CustomInspectorWindow {
  fn new(title: String) -> Self {
    CustomInspectorWindow {
      title,
      rows: Vec::new(),
    }
  }

  fn add_instance(&mut self, label: String, instance: GameInstance) {
    self
      .rows
      .push(CustomInspectorRow::Instance { label, instance });
  }

  fn add_text(&mut self, text: String) {
    self.rows.push(CustomInspectorRow::Text(text));
  }
}

// ---------------------------------------------------------------------------
// The call-scoped environment bridge
// ---------------------------------------------------------------------------

/// Type-erased pointers to the frame's `Ctx` and object map. Reconstituted by
/// [`with_env`]; see the module docs for why this is sound.
#[derive(Clone, Copy)]
struct ScriptEnv {
  ctx: *const (),
  objects: *const (),
}

thread_local! {
  /// `Some` only while a [`ScriptFrame`] is alive on this thread.
  static ENV: Cell<Option<ScriptEnv>> = const { Cell::new(None) };
  /// Windows submitted via `show(w)` during the current script run.
  static SINK: RefCell<Vec<CustomInspectorWindow>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard that publishes `ctx` + `objects` to the thread-local for the
/// duration of one or more `engine.run_ast` calls, then tears them down.
///
/// The lifetime `'a` ties the guard to the borrows it publishes; nothing hands
/// out a `ScriptFrame` that outlives them.
pub struct ScriptFrame<'a> {
  _marker: PhantomData<&'a ()>,
}

impl<'a> ScriptFrame<'a> {
  pub fn enter<'c>(ctx: &'a Ctx<'c>, objects: &'a BTreeMap<TUniqueID, GameInstance>) -> Self
  where
    'c: 'a,
  {
    let env = ScriptEnv {
      // Type/lifetime-erase for storage. `with_env` reconstitutes a borrow that
      // cannot outlive this guard
      ctx: ctx as *const Ctx<'c> as *const (),
      objects: objects as *const BTreeMap<TUniqueID, GameInstance> as *const (),
    };
    ENV.with(|e| e.set(Some(env)));
    SINK.with(|s| s.borrow_mut().clear());
    ScriptFrame {
      _marker: PhantomData,
    }
  }

  pub fn take_windows(&self) -> Vec<CustomInspectorWindow> {
    SINK.with(|s| std::mem::take(&mut *s.borrow_mut()))
  }
}

impl Drop for ScriptFrame<'_> {
  fn drop(&mut self) {
    ENV.with(|e| e.set(None));
    SINK.with(|s| s.borrow_mut().clear());
  }
}

fn no_env() -> Box<EvalAltResult> {
  Box::new(EvalAltResult::ErrorRuntime(
    "primewatch scripting functions can only be called while a script is running".into(),
    Position::NONE,
  ))
}

/// Run `f` against the live `Ctx` + object map published by the enclosing
/// [`ScriptFrame`]. Errors (as a script runtime error) if called outside one.
fn with_env<R>(
  f: impl FnOnce(&Ctx, &BTreeMap<TUniqueID, GameInstance>) -> R,
) -> Result<R, Box<EvalAltResult>> {
  ENV.with(|e| {
    let env = e.get().ok_or_else(no_env)?;
    // SAFETY: `ENV` is `Some` only between `ScriptFrame::enter` and its `Drop`.
    // That guard borrows both referents for `'a`, and script execution is
    // synchronous on this thread, so both pointers are live for the call to
    // `f`. `f` returns an owned `R`; no borrow of `*ctx` / `*objects` escapes.
    let ctx = unsafe { &*(env.ctx as *const Ctx) };
    let objects = unsafe { &*(env.objects as *const BTreeMap<TUniqueID, GameInstance>) };
    Ok(f(ctx, objects))
  })
}

// ---------------------------------------------------------------------------
// Small conversion helpers
// ---------------------------------------------------------------------------

fn opt_instance(inst: Option<GameInstance>) -> Dynamic {
  inst.map(Dynamic::from).unwrap_or(Dynamic::UNIT)
}

fn opt_dyn<T: Into<Dynamic>>(v: Option<T>) -> Dynamic {
  v.map(Into::into).unwrap_or(Dynamic::UNIT)
}

// ---------------------------------------------------------------------------
// Engine construction
// ---------------------------------------------------------------------------

pub fn build_engine() -> Engine {
  let mut engine = Engine::new();

  // A script re-runs every frame on the render thread: cap it so a runaway
  // loop can't wedge the UI. ~1M ops is generous for an inspector script.
  engine.set_max_operations(1_000_000);
  engine.set_max_call_levels(64);
  engine.set_max_string_size(64 * 1024);
  engine.set_max_array_size(64 * 1024);

  engine.register_type_with_name::<GameInstance>("GameInstance");
  engine.register_type_with_name::<CustomInspectorWindow>("InspectorWindow");

  register_window_api(&mut engine);
  register_lookup_api(&mut engine);
  register_instance_api(&mut engine);

  engine
}

fn register_window_api(engine: &mut Engine) {
  engine.register_fn("inspector_window", CustomInspectorWindow::new);

  // add(w, label, <instance>) — a live, re-read-every-frame tree node
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, label: String, inst: GameInstance| {
      w.add_instance(label, inst);
    },
  );
  // add(w, <instance>) — same, unlabelled (uses the type name as caption)
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, inst: GameInstance| {
      let label = inst.type_name.to_string();
      w.add_instance(label, inst);
    },
  );
  // add(w, text) — a literal line
  engine.register_fn("add", |w: &mut CustomInspectorWindow, text: String| {
    w.add_text(text);
  });
  // add(w, label, value) — a "label: value" snapshot line
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, label: String, value: Dynamic| {
      w.add_text(format!("{label}: {value}"));
    },
  );

  // Submit a window to be drawn this frame.
  engine.register_fn("show", |w: CustomInspectorWindow| {
    SINK.with(|s| s.borrow_mut().push(w));
  });
}

fn register_lookup_api(engine: &mut Engine) {
  // Fixed global roots — no memory read, always succeed.
  engine.register_fn("get_state_manager", get_state_manager);
  engine.register_fn("get_main", get_main);

  // `g_stateManager.player` (auto-derefs the `*CPlayer`). Unit if unreadable.
  engine.register_fn(
    "get_player_entity",
    || -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt_instance(get_state_manager().get_member(ctx, "player")))
    },
  );

  engine.register_fn(
    "get_entity_by_unique_id",
    |id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|_, objects| opt_instance(objects.get(&(id as u16)).cloned()))
    },
  );

  engine.register_fn(
    "get_entity_by_editor_id",
    |editor_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, objects| {
        let target = editor_id as u32;
        let found = objects
          .values()
          .find(|e| e.get_member(ctx, "editorID").and_then(|m| m.read_u32(ctx)) == Some(target));
        opt_instance(found.cloned())
      })
    },
  );
}

fn register_instance_api(engine: &mut Engine) {
  // gi["member"] and gi.member("name") — resolve a struct member (auto-deref of
  // pointer members is handled by `GameInstance::get_member`). Unit if absent.
  engine.register_indexer_get(
    |inst: &mut GameInstance, name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt_instance(inst.get_member(ctx, name)))
    },
  );
  engine.register_fn(
    "member",
    |inst: &mut GameInstance, name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| opt_instance(inst.get_member(ctx, name)))
    },
  );

  // gi.elem(i) — array element handle (bit/array metadata cleared, per `element`).
  engine.register_fn(
    "elem",
    |inst: &mut GameInstance, index: i64| -> Result<Dynamic, Box<EvalAltResult>> {
      with_env(|ctx, _| Dynamic::from(inst.element(ctx, index.max(0) as u32)))
    },
  );

  engine.register_get("address", |inst: &mut GameInstance| inst.address as i64);
  engine.register_get("type_name", |inst: &mut GameInstance| {
    inst.type_name.to_string()
  });
  engine.register_fn("to_string", |inst: &mut GameInstance| {
    format!("{} @ {:#010x}", inst.type_name, inst.address)
  });

  engine.register_fn(
    "extends",
    |inst: &mut GameInstance, class: &str| -> Result<bool, Box<EvalAltResult>> {
      with_env(|ctx, _| inst.extends_class(ctx, class))
    },
  );

  // ---- typed reads: value, or unit when the address is unreadable ----
  macro_rules! read_fn {
    ($name:literal, $body:expr) => {
      engine.register_fn(
        $name,
        |inst: &mut GameInstance| -> Result<Dynamic, Box<EvalAltResult>> {
          #[allow(clippy::redundant_closure_call)]
          with_env(|ctx, _| $body(inst, ctx))
        },
      );
    };
  }
  read_fn!("read_f32", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_f32(c).map(|v| v as f64)
  ));
  read_fn!("read_f64", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_f64(c)
  ));
  read_fn!("read_bool", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_bool(c)
  ));
  read_fn!("read_string", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_string(c)
  ));
  read_fn!("read_i8", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u8(c).map(|v| v as i8 as i64)
  ));
  read_fn!("read_i16", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u16(c).map(|v| v as i16 as i64)
  ));
  read_fn!("read_i32", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u32(c).map(|v| v as i32 as i64)
  ));
  read_fn!("read_i64", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u64(c).map(|v| v as i64)
  ));
  read_fn!("read_u8", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u8(c).map(|v| v as i64)
  ));
  read_fn!("read_u16", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u16(c).map(|v| v as i64)
  ));
  read_fn!("read_u32", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u32(c).map(|v| v as i64)
  ));
  read_fn!("read_u64", |i: &GameInstance, c: &Ctx| opt_dyn(
    i.read_u64(c).map(|v| v as i64)
  ));
}

// ---------------------------------------------------------------------------
// Script discovery / per-frame execution
// ---------------------------------------------------------------------------

const SCRIPT_DIR: &str = "scripts";

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
      let enabled = previously_enabled.get(&name).copied().unwrap_or(true);
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::GameStructs;

  fn load_defs() -> GameStructs {
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    structs
  }

  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping scripting mem1.raw test: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  fn run(
    src: &str,
    ctx: &Ctx,
    objects: &BTreeMap<TUniqueID, GameInstance>,
  ) -> Result<Vec<CustomInspectorWindow>, String> {
    let engine = build_engine();
    let ast = engine.compile(src).map_err(|e| e.to_string())?;
    let frame = ScriptFrame::enter(ctx, objects);
    engine.run_ast(&ast).map_err(|e| e.to_string())?;
    Ok(frame.take_windows())
  }

  #[test]
  fn functions_error_outside_a_script_frame() {
    // No `ScriptFrame` alive -> the bridge refuses rather than dereferencing a
    // null pointer.
    let engine = build_engine();
    let err = engine
      .eval::<Dynamic>("get_player_entity()")
      .unwrap_err()
      .to_string();
    assert!(err.contains("script"), "unexpected error: {err}");
  }

  #[test]
  fn missing_member_is_unit_not_a_panic() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let objects = BTreeMap::new();

    let windows = run(
      r#"
        let sm = get_state_manager();
        let w = inspector_window("t");
        w.add("bogus is null", (sm["no_such_member"] == ()).to_string());
        show(w);
      "#,
      &ctx,
      &objects,
    )
    .expect("run");
    assert_eq!(windows.len(), 1);
    match &windows[0].rows[0] {
      CustomInspectorRow::Text(t) => assert_eq!(t, "bogus is null: true"),
      other => panic!("unexpected row: {other:?}", other = row_kind(other)),
    }
  }

  fn row_kind(row: &CustomInspectorRow) -> &'static str {
    match row {
      CustomInspectorRow::Instance { .. } => "Instance",
      CustomInspectorRow::Text(_) => "Text",
    }
  }

  #[test]
  fn reads_player_sj_timer_from_live_dump() {
    let Some(mem) = load_mem1() else { return };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);
    let objects = crate::mem::game_object_utils::get_all_objects(&ctx);

    let windows = run(
      r#"
        let p = get_player_entity();
        let w = inspector_window("Player");
        w.add("sjTimer", p["sjTimer"]);
        w.add("sjTimer value", p["sjTimer"].read_f32());
        w.add("is CActor", p.extends("CActor").to_string());
        show(w);
      "#,
      &ctx,
      &objects,
    )
    .expect("run");

    assert_eq!(windows.len(), 1);
    let w = &windows[0];

    // Row 0: a live handle at CPlayer + 0x28C, typed f32.
    let player = get_state_manager()
      .get_member(&ctx, "player")
      .expect("player handle");
    match &w.rows[0] {
      CustomInspectorRow::Instance { label, instance } => {
        assert_eq!(label, "sjTimer");
        assert_eq!(instance.address, player.address + 0x28C);
        assert_eq!(instance.type_name.as_ref(), "f32");
      }
      other => panic!("row 0 kind: {}", row_kind(other)),
    }

    // Row 1: the snapshot value matches a direct read.
    let expected = format!(
      "sjTimer value: {}",
      Dynamic::from(mem.read_f32(player.address + 0x28C).unwrap() as f64)
    );
    match &w.rows[1] {
      CustomInspectorRow::Text(t) => assert_eq!(*t, expected),
      other => panic!("row 1 kind: {}", row_kind(other)),
    }

    // Row 2: CPlayer extends CActor transitively.
    match &w.rows[2] {
      CustomInspectorRow::Text(t) => assert_eq!(t, "is CActor: true"),
      other => panic!("row 2 kind: {}", row_kind(other)),
    }
  }

  #[test]
  fn env_is_cleared_after_the_frame_guard_drops() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let objects = BTreeMap::new();
    {
      let _frame = ScriptFrame::enter(&ctx, &objects);
      assert!(with_env(|_, _| ()).is_ok());
    }
    assert!(with_env(|_, _| ()).is_err());
  }
}
