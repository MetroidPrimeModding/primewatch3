//! Ports `../primewatch2/src/utils/GameObjectUtils.cpp` — walks the
//! `CObjectList` hanging off `g_stateManager` into a `TUniqueID -> GameInstance`
//! map, refreshed once per frame by the app shell.
//!
//! Only `getAllObjects` / `getObjectByEntityID` are ported here. The resource
//! browser helpers (`getAllCObjectReferences` / `getAllLoadingDatas`) and the
//! renderer string helpers (`objectTagToString` / `fourCCToString`) belong to
//! later phases (P7 / P8).

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::mem::globals::get_state_manager;
use crate::mem::vtables::vtable_class_name;
use crate::structs::prime_structs::GameInstance;

/// C++ `TUniqueID` — the per-area object id used as the `CObjectList` slot index.
pub type TUniqueID = u16;

/// Sentinel terminating the intrusive slot list (C++ `0xFFFF`).
const LIST_END: u16 = 0xFFFF;

/// Ports `GameObjectUtils::getAllObjects` (`GameObjectUtils.cpp:26-61`).
///
/// Reads `g_stateManager["allObjects"]` (auto-derefs `*CObjectList`), then walks
/// the intrusive linked list of `SObjectListEntry` slots starting at `firstID`,
/// stopping at `0xFFFF` or after `size + 1` iterations (the C++ "bad timing"
/// emergency break). Each slot's `entity` (`*CEntity` auto-deref) is retyped to
/// its concrete class when the vtable at `+0x0` is in `MP1_VTABLES` *and* the
/// mapped name is a real `.bs` struct (`GameVtables.cpp` parity).
///
/// Deviation from C++: C++ reads are total (`getRealPtr` clamps OOB to 0). Per
/// the P4.2 / P5.1 convention these reads are `Option` with no default
/// substitution, so a `None` on any structural/value read mid-walk stops the
/// walk and returns what was gathered so far rather than fabricating a `0`. With
/// a valid snapshot this never triggers.
pub fn get_all_objects(ctx: &Ctx) -> HashMap<TUniqueID, GameInstance> {
  let mut objects: HashMap<TUniqueID, GameInstance> = HashMap::new();

  let Some(global_list) = get_state_manager().get_member(ctx, "allObjects") else {
    return objects;
  };
  let Some(first) = global_list
    .get_member(ctx, "firstID")
    .and_then(|m| m.read_u16(ctx))
  else {
    return objects;
  };
  let Some(size) = global_list
    .get_member(ctx, "size")
    .and_then(|m| m.read_u16(ctx))
  else {
    return objects;
  };
  let size = size.min(1024);
  let Some(list) = global_list.get_member(ctx, "list") else {
    return objects;
  };

  let mut count: u32 = 0;
  let mut current_id: u16 = first;
  while current_id != LIST_END {
    // Emergency exit in case of bad timing (C++ `if (count > size) break;`).
    if count > size as u32 {
      break;
    }
    count += 1;

    let current_entry = list.element(ctx, current_id as u32);
    let Some(mut entity) = current_entry.get_member(ctx, "entity") else {
      break;
    };
    let Some(vtable) = entity
      .get_member(ctx, "vtable")
      .and_then(|m| m.read_u32(ctx))
    else {
      break;
    };

    // Retype only when the vtable is in `MP1_VTABLES` *and* the mapped name is a
    // real `.bs` struct (`GameVtables.cpp` parity).
    if let Some(name) =
      vtable_class_name(vtable).filter(|name| ctx.structs.get_struct_by_name(name).is_some())
    {
      entity.type_name = name.to_string();
    }

    objects.insert(current_id, entity);

    // Advance via the *indexed* slot's `next` link (C++ reads `currentEntry`,
    // and `prev` / `next` are per-slot).
    let Some(next) = current_entry
      .get_member(ctx, "next")
      .and_then(|m| m.read_u16(ctx))
    else {
      break;
    };
    current_id = next;
  }

  objects
}

/// Ports `GameObjectUtils::getObjectByEntityID` (`GameObjectUtils.cpp:13-24`).
///
/// Single-slot lookup into the same `CObjectList`: `eid & 0x3FF` is the slot
/// index; returns that slot's `entity` (`*CEntity` auto-deref), with no vtable
/// retype. Needed by the world renderer's camera lookup (`WorldRenderer.cpp:143`).
pub fn get_object_by_entity_id(ctx: &Ctx, eid: u16) -> Option<GameInstance> {
  let actual_id = eid & 0x3FF;
  let global_list = get_state_manager().get_member(ctx, "allObjects")?;
  let list = global_list.get_member(ctx, "list")?;
  list
    .element(ctx, actual_id as u32)
    .get_member(ctx, "entity")
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
  /// `game_memory.rs` / `prime_structs.rs` tests).
  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/../primewatch2/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping game_object_utils mem1.raw tests: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  #[test]
  fn object_list_entry_resolves_as_struct_with_stride_8() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    // A bare handle onto the slot type: `element_size` must be the struct size
    // (0x8), not the `primitive_size` fallback of 4.
    let entry = GameInstance::new(0x8000_0000, "SObjectListEntry".to_string());
    assert_eq!(entry.element_size(&ctx), 0x8);
  }

  #[test]
  fn get_all_objects_walks_the_live_list() {
    let Some(mem) = load_mem1() else { return };
    let structs = load_defs();
    let ctx = Ctx::new(&structs, &mem);

    let objects = get_all_objects(&ctx);
    assert!(!objects.is_empty(), "expected a non-empty object list");

    for inst in objects.values() {
      // Every entity address is a `0x8...` effective address that lands inside
      // the snapshot. (The task's suggested `addr & 0xFF00_0000 == 0x8000_0000`
      // is too tight — MEM1 spans 0x80000000..0x81800000, and real entities live
      // above 0x81000000 — so verify readability instead.)
      assert_eq!(inst.address & 0x8000_0000, 0x8000_0000);
      assert!(
        ctx.mem.read_u32(inst.address).is_some(),
        "object at 0x{:08x} not readable in the snapshot",
        inst.address
      );
    }

    let retyped = objects
      .values()
      .filter(|i| i.type_name != "CEntity")
      .count();
    assert!(
      retyped > 0,
      "expected at least one entity retyped by vtable"
    );

    // A key from the map resolves to the same entity address via the id path.
    let (&id, inst) = objects.iter().next().unwrap();
    let looked_up = get_object_by_entity_id(&ctx, id).expect("lookup by id");
    assert_eq!(looked_up.address, inst.address);
  }

  #[test]
  fn get_all_objects_on_zeroed_memory_does_not_panic() {
    let structs = load_defs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let _ = get_all_objects(&ctx);
  }
}
