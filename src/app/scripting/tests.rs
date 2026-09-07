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
