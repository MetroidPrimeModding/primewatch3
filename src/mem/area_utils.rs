//! Ports `../primewatch2/src/utils/AreaUtils.cpp`.
//!
//! Walks `g_stateManager -> world -> areas` (an
//! `rstl::vector<rstl::autoptr<CGameArea>>`) and hands back one `CGameArea`
//! handle per live area, in area-index order.

use crate::ctx::Ctx;
use crate::mem::globals::get_state_manager;
use crate::structs::prime_structs::GameInstance;

/// Defensive upper bound on the area count.
///
/// Deviation from C++ `AreaUtils::getAreas`, which loops `for (int i = 0; i <
/// end; i++)` with no failsafe — a garbage `end` from an unloaded / partway
/// world would spin the frame. Mirrors the 1024 clamp `GameObjectUtils::
/// getAllObjects` already applies to its list size.
const AREA_CAP: u32 = 1024;

/// Ports `AreaUtils::getAreas` (`AreaUtils.cpp:8-27`).
///
/// `g_stateManager["world"]` (auto-derefs `*CWorld`) -> `["areas"]` (the
/// `rstl::vector<rstl::autoptr<CGameArea>>` value member at `CWorld` +0x18).
/// `areas["end"]` is — despite the name — the element count (`rstl.bs`
/// `rstl::vector<T> { u32 end; u32 size; *T first }`). `areas["first"]`
/// auto-derefs `*T` to a handle of type `rstl::autoptr<CGameArea>`; each
/// element is `sizeof(rstl::autoptr<T>)` = 0x8 apart, and `["value"]`
/// auto-derefs `*CGameArea` to the area handle.
///
/// Deviations from C++:
/// - C++ sets `area.name = fmt::format("area {}", i)`. `GameInstance` has no
///   `name` field (P6.1 precedent); the returned `Vec` index *is* the label and
///   formatting it is a P7 render-layer concern.
/// - The loop is bounded by [`AREA_CAP`] (see its docs).
/// - Per the P4.2 / P5.1 convention these reads are `Option` with no default
///   substitution: a `None` on any structural / value read bails with what was
///   gathered so far rather than fabricating a `0`. With a valid snapshot this
///   never triggers.
pub fn get_areas(ctx: &Ctx) -> Vec<GameInstance> {
  let sm = get_state_manager();
  let Some(world) = sm.get_member(ctx, "world") else {
    return vec![];
  };
  let Some(areas) = world.get_member(ctx, "areas") else {
    return vec![];
  };
  let Some(end) = areas.get_member(ctx, "end").and_then(|e| e.read_u32(ctx)) else {
    return vec![];
  };
  let Some(first) = areas.get_member(ctx, "first") else {
    return vec![];
  };

  let mut result = Vec::new();
  for i in 0..end.min(AREA_CAP) {
    let item = first.element(ctx, i);
    if let Some(area) = item.get_member(ctx, "value") {
      result.push(area);
    }
  }
  result
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::GameStructs;

  /// Real `.bs` schema from this crate's `prime_defs/`.
  fn load_defs() -> GameStructs {
    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"))
      .expect("load prime_defs");
    structs
  }

  /// Skip-if-absent loader for the offline BE dump (same contract as the
  /// `game_object_utils.rs` / `globals.rs` tests).
  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/../primewatch2/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping area_utils mem1.raw tests: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  #[test]
  fn autoptr_element_stride_is_8() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let first = GameInstance::new(0x8000_0000, "rstl::autoptr<CGameArea>".to_string());
    assert_eq!(first.element_size(&ctx), 8);
  }

  #[test]
  fn get_areas_reads_the_live_world() {
    let Some(mem) = load_mem1() else { return };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);

    let areas = get_areas(&ctx);
    assert!(!areas.is_empty(), "expected a non-empty area list");

    for area in &areas {
      assert_eq!(area.type_name, "CGameArea");
      assert_eq!(area.address & 0x8000_0000, 0x8000_0000);
      assert!(
        ctx.mem.read_u32(area.address).is_some(),
        "area at 0x{:08x} not readable in the snapshot",
        area.address
      );
    }
  }

  #[test]
  fn get_areas_on_zeroed_memory_does_not_panic() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    assert!(get_areas(&ctx).is_empty());
  }
}
