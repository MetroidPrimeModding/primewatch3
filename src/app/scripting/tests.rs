use std::collections::BTreeMap;

use rhai::Dynamic;

use crate::ctx::Ctx;
use crate::mem::game_memory::GameMemory;
use crate::mem::game_object_utils::TUniqueID;
use crate::mem::globals::get_state_manager;
use crate::structs::prime_structs::{GameInstance, GameStructs};

use super::engine::build_engine;
use super::env::{ScriptFrame, with_env};
use super::manager::SCRIPT_DIR;
use super::window::{AnchorAlign, CustomInspectorRow, CustomInspectorWindow};

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

fn row_kind(row: &CustomInspectorRow) -> &'static str {
  match row {
    CustomInspectorRow::Instance { .. } => "Instance",
    CustomInspectorRow::Text(_) => "Text",
  }
}

#[test]
fn packaged_default_enabled_state_parses_and_matches_shipped_scripts() {
  let root = concat!(env!("CARGO_MANIFEST_DIR"));
  let text = std::fs::read_to_string(format!("{root}/packaging/primewatch_scripts.ron"))
    .expect("read packaging/primewatch_scripts.ron");
  let map: BTreeMap<String, bool> =
    ron::from_str(&text).expect("packaged enabled state is valid RON");

  // Every key must name a script actually shipped in `scripts/`, so the
  // default can't silently drift from the release contents.
  for name in map.keys() {
    let path = format!("{root}/{SCRIPT_DIR}/{name}");
    assert!(
      std::path::Path::new(&path).exists(),
      "packaged enabled state references missing script {name}"
    );
  }
  assert_eq!(map.get("player_status.rhai"), Some(&true));
  assert_eq!(map.get("example.rhai"), Some(&false));
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

#[test]
fn window_hide_title_and_anchor_set_the_expected_fields() {
  let structs = load_defs();
  let mem = GameMemory::new();
  let ctx = Ctx::new(&structs, &mem);
  let objects = BTreeMap::new();

  let windows = run(
    r#"
      let w = inspector_window("HUD");
      w.hide_title();
      w.anchor("left_bottom", 8.0, -8.0);
      show(w);
    "#,
    &ctx,
    &objects,
  )
  .expect("run");

  assert_eq!(windows.len(), 1);
  let w = &windows[0];
  assert!(!w.title_bar);
  let anchor = w.anchor.expect("anchor set");
  assert_eq!(anchor.align, (AnchorAlign::Min, AnchorAlign::Max));
  assert_eq!(anchor.offset, (8.0, -8.0));
}

#[test]
fn window_anchor_rejects_an_unknown_alignment_name() {
  let structs = load_defs();
  let mem = GameMemory::new();
  let ctx = Ctx::new(&structs, &mem);
  let objects = BTreeMap::new();

  let err = match run(
    r#"
      let w = inspector_window("HUD");
      w.anchor("nowhere", 0.0, 0.0);
      show(w);
    "#,
    &ctx,
    &objects,
  ) {
    Ok(_) => panic!("expected an error"),
    Err(err) => err,
  };
  assert!(err.contains("unknown anchor"), "unexpected error: {err}");
}

// ---------------------------------------------------------------------------
// glam Vec3 / Quat / Mat4
// ---------------------------------------------------------------------------

/// Collect the `Text` rows of the single window a script submits.
fn text_rows(src: &str) -> Vec<String> {
  let structs = load_defs();
  let mem = GameMemory::new();
  let ctx = Ctx::new(&structs, &mem);
  let objects = BTreeMap::new();
  let windows = run(src, &ctx, &objects).expect("run");
  assert_eq!(windows.len(), 1);
  windows[0]
    .rows
    .iter()
    .map(|r| match r {
      CustomInspectorRow::Text(t) => t.clone(),
      other => panic!("unexpected row: {}", row_kind(other)),
    })
    .collect()
}

#[test]
fn vec3_constructors_operators_and_methods() {
  let rows = text_rows(
    r#"
      let a = vec3(1.0, 2.0, 3.0);
      let b = vec3(4, 5, 6);           // INT args coerce
      let w = inspector_window("v");
      w.add("sum", a + b);
      w.add("scaled", a * 2.0);
      w.add("scaled_int", 2 * a);
      w.add("neg", -a);
      w.add("dot", (a.dot(b)).to_string());
      w.add("cross", a.cross(b));
      w.add("len", (vec3(3.0, 4.0, 0.0).length()).to_string());
      w.add("eq", (vec3(1, 1, 1) == vec3(1.0, 1.0, 1.0)).to_string());
      w.add("interp", `${a}`);
      show(w);
    "#,
  );
  assert_eq!(rows[0], "sum: (5, 7, 9)");
  assert_eq!(rows[1], "scaled: (2, 4, 6)");
  assert_eq!(rows[2], "scaled_int: (2, 4, 6)");
  assert_eq!(rows[3], "neg: (-1, -2, -3)");
  assert_eq!(rows[4], "dot: 32.0");
  assert_eq!(rows[5], "cross: (-3, 6, -3)");
  assert_eq!(rows[6], "len: 5.0");
  assert_eq!(rows[7], "eq: true");
  assert_eq!(rows[8], "interp: (1, 2, 3)");
}

#[test]
fn quat_identity_rotation_and_compose() {
  let rows = text_rows(
    r#"
      let id = quat_identity();
      let v = vec3(1.0, 0.0, 0.0);
      let w = inspector_window("q");
      w.add("id_rot", id * v);
      // 180 deg about Z sends +X to -X.
      let flip = quat_from_axis_angle(vec3(0.0, 0.0, 1.0), 3.14159265358979);
      let r = flip.rotate_vec3(v);
      w.add("flip_x", (r.x).to_string());
      w.add("compose_is_id", ((flip * flip.inverse()) == id).to_string());
      show(w);
    "#,
  );
  assert_eq!(rows[0], "id_rot: (1, 0, 0)");
  assert!(
    rows[1].starts_with("flip_x: -0.999") || rows[1].starts_with("flip_x: -1"),
    "unexpected: {}",
    rows[1]
  );
  assert_eq!(rows[2], "compose_is_id: true");
}

#[test]
fn mat4_translation_transform_and_inverse() {
  let rows = text_rows(
    r#"
      let t = mat4_from_translation(vec3(10.0, 0.0, 0.0));
      let w = inspector_window("m");
      w.add("moved", t * vec3(1.0, 2.0, 3.0));
      w.add("translation", t.translation());
      w.add("round_trip", (t * t.inverse() == mat4_identity()).to_string());
      show(w);
    "#,
  );
  assert_eq!(rows[0], "moved: (11, 2, 3)");
  assert_eq!(rows[1], "translation: (10, 0, 0)");
  assert_eq!(rows[2], "round_trip: true");
}

#[test]
fn shipped_scripts_compile() {
  let engine = build_engine();
  let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/", "scripts");
  for entry in std::fs::read_dir(dir).expect("read scripts/") {
    let path = entry.expect("dir entry").path();
    if path.extension().and_then(|e| e.to_str()) != Some("rhai") {
      continue;
    }
    let src = std::fs::read_to_string(&path).expect("read script");
    engine
      .compile(&src)
      .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
  }
}

#[test]
fn player_status_script_runs_against_the_live_dump() {
  let Some(mem) = load_mem1() else { return };
  let structs = load_defs();
  let ctx = Ctx::new(&structs, &mem);
  let objects = crate::mem::game_object_utils::get_all_objects(&ctx);

  let src = std::fs::read_to_string(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/player_status.rhai"
  ))
  .expect("read player_status.rhai");

  let windows = run(&src, &ctx, &objects).expect("run");
  assert_eq!(windows.len(), 1, "player_status submits one window");
  let rows: Vec<&str> = windows[0]
    .rows
    .iter()
    .map(|r| match r {
      CustomInspectorRow::Text(t) => t.as_str(),
      other => panic!("unexpected row: {}", row_kind(other)),
    })
    .collect();
  assert_eq!(rows.len(), 3);
  assert!(rows[0].starts_with("pos: "), "row 0: {}", rows[0]);
  assert!(rows[1].starts_with("vel: "), "row 1: {}", rows[1]);
  assert!(rows[2].starts_with("look: "), "row 2: {}", rows[2]);

  // Position matches a direct scalar read of the CTransform pos fields.
  let player = get_state_manager()
    .get_member(&ctx, "player")
    .expect("player");
  let xf = player.get_member(&ctx, "transform").expect("transform");
  let px = xf
    .get_member(&ctx, "posX")
    .and_then(|m| m.read_f32(&ctx))
    .expect("posX");
  let want = (px * 1000.0).round() / 1000.0;
  assert!(
    rows[0].contains(&format!("pos: {want}x")),
    "row 0 {} vs posX {want}",
    rows[0]
  );
}

#[test]
fn typed_reads_off_the_live_dump() {
  let Some(mem) = load_mem1() else { return };
  let structs = load_defs();
  let ctx = Ctx::new(&structs, &mem);
  let objects = crate::mem::game_object_utils::get_all_objects(&ctx);

  let windows = run(
    r#"
      let p = get_player_entity();
      let w = inspector_window("Player");
      let vel = p.velocity.read_vec3();
      let xf = p.transform.read_transform();
      w.add("vel is vec3", (vel != ()).to_string());
      w.add("vel matches x", (vel.x == p.velocity.x.read_f32()).to_string());
      w.add("xf translation matches posX",
        (xf.translation().x == p.transform.posX.read_f32()).to_string());
      show(w);
    "#,
    &ctx,
    &objects,
  )
  .expect("run");

  let rows: Vec<&String> = windows[0]
    .rows
    .iter()
    .map(|r| match r {
      CustomInspectorRow::Text(t) => t,
      other => panic!("unexpected row: {}", row_kind(other)),
    })
    .collect();
  assert_eq!(rows[0], "vel is vec3: true");
  assert_eq!(rows[1], "vel matches x: true");
  assert_eq!(rows[2], "xf translation matches posX: true");
}
