//! [`build_engine`] and the rhai function registrations that make up the script
//! API: window building, global lookups, and `GameInstance` member access /
//! typed reads.

use rhai::{Dynamic, Engine, EvalAltResult, Position};

use crate::ctx::Ctx;
use crate::mem::globals::{get_main, get_state_manager};
use crate::structs::prime_structs::GameInstance;

use super::env::{submit_window, with_env};
use super::window::{CustomInspectorWindow, WindowAnchor, parse_anchor};

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

pub(super) fn build_engine() -> Engine {
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
  super::math::register_math_api(&mut engine);

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
  // add(w, label, <glam value>) — the catch-all `Dynamic` overload above would
  // print the opaque type name, so format these explicitly.
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, label: String, v: glam::Vec3| {
      w.add_text(format!("{label}: ({}, {}, {})", v.x, v.y, v.z));
    },
  );
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, label: String, q: glam::Quat| {
      w.add_text(format!("{label}: ({}, {}, {}, {})", q.x, q.y, q.z, q.w));
    },
  );
  engine.register_fn(
    "add",
    |w: &mut CustomInspectorWindow, label: String, m: glam::Mat4| {
      w.add_text(format!("{label}: {m}"));
    },
  );

  // Hide the title bar (and with it, dragging/collapsing) — for a fixed HUD
  // overlay rather than a draggable inspector panel.
  engine.register_fn("hide_title", |w: &mut CustomInspectorWindow| {
    w.title_bar = false;
  });

  // Pin the window to a screen corner/edge/center, e.g.
  // `w.anchor("left_bottom", 8.0, -8.0)`. `align` is one of left/center/right
  // crossed with top/center/bottom, joined with "_".
  engine.register_fn(
    "anchor",
    |w: &mut CustomInspectorWindow,
     align: &str,
     offset_x: f64,
     offset_y: f64|
     -> Result<(), Box<EvalAltResult>> {
      let align = parse_anchor(align)
        .map_err(|e| Box::new(EvalAltResult::ErrorRuntime(e.into(), Position::NONE)))?;
      w.anchor = Some(WindowAnchor {
        align,
        offset: (offset_x as f32, offset_y as f32),
      });
      Ok(())
    },
  );

  // Submit a window to be drawn this frame.
  engine.register_fn("show", |w: CustomInspectorWindow| submit_window(w));
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
