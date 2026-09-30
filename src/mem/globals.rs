//! The global roots the app traverses from every frame, at the running revision's
//! addresses (`Ctx::symbol`). `None` when this revision has no address for the root.
//!
//! The non-pointer roots (`CStateManager`, `CMain`) are statically allocated, so the
//! symbol is the object. The pointer roots (`gpMemoryCard`, `gpTweakPlayer`) hold a
//! pointer to it, and a failed read of that pointer is also `None`.

use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;
use std::string::ToString;

pub fn get_state_manager(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.symbol("sAllocSpace$CStateManager")?;
  Some(GameInstance::new(address, "CStateManager".to_string()))
}

pub fn get_main(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.symbol("sMainSpace")?;
  Some(GameInstance::new(address, "CMain".to_string()))
}

pub fn get_memory_card(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.mem.read_u32(ctx.symbol("gpMemoryCard")?)?;
  Some(GameInstance::new(address, "CMemoryCardSys".to_string()))
}

pub fn get_tweak_player(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.mem.read_u32(ctx.symbol("gpTweakPlayer")?)?;
  Some(GameInstance::new(address, "CTweakPlayer".to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::mem::game_version::GameVersion;
  use crate::mem::symbols::SymbolTable;
  use crate::structs::prime_structs::GameStructs;
  use std::rc::Rc;

  /// No `.bs` structs, just `version`'s symbols.
  fn symbols_only(version: GameVersion) -> GameStructs {
    let defs_dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"));
    let mut structs = GameStructs::new_empty();
    structs.version = version;
    structs.symbols = SymbolTable::load(defs_dir, version).unwrap().map(Rc::new);
    structs
  }

  #[test]
  fn roots_resolve_per_revision() {
    let structs = symbols_only(GameVersion::NtscU0_00);
    let mut mem = GameMemory::new();
    mem.data[0x805A8C44 & 0x7FFFFFFF..][..4].copy_from_slice(&0x8123_4560u32.to_be_bytes());
    let ctx = Ctx::new(&structs, &mem, GameVersion::NtscU0_00);

    let sm = get_state_manager(&ctx).unwrap();
    assert_eq!(sm.address, 0x8045A1A8);
    assert_eq!(sm.type_name.as_ref(), "CStateManager");
    let main = get_main(&ctx).unwrap();
    assert_eq!(main.address, 0x80457560);
    assert_eq!(main.type_name.as_ref(), "CMain");
    let card = get_memory_card(&ctx).unwrap();
    assert_eq!(card.address, 0x8123_4560);
    assert_eq!(card.type_name.as_ref(), "CMemoryCardSys");

    let pal_structs = symbols_only(GameVersion::Pal);
    if pal_structs.symbols.is_some() {
      let pal = Ctx::new(&pal_structs, &mem, GameVersion::Pal);
      assert_eq!(get_state_manager(&pal).unwrap().address, 0x803E2088);
      assert_eq!(get_main(&pal).unwrap().address, 0x803DF440);
    }

    let wii_structs = symbols_only(GameVersion::TrilogyNtsc);
    let wii = Ctx::new(&wii_structs, &mem, GameVersion::TrilogyNtsc);
    assert!(get_state_manager(&wii).is_none());
    assert!(get_tweak_player(&wii).is_none());

    // Structs still loaded for another revision: no addresses rather than wrong ones.
    let stale = Ctx::new(&structs, &mem, GameVersion::Pal);
    assert!(get_state_manager(&stale).is_none());
  }

  #[test]
  fn pointer_roots_deref_live_dump_when_present() {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping globals mem1.raw test: {path} not found");
      return;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    let version = GameVersion::detect(&mem).expect("mem1.raw is a Prime dump");
    let structs = symbols_only(version);
    let ctx = Ctx::new(&structs, &mem, version);

    // Both globals should point somewhere inside the emulated RAM window.
    for inst in [get_memory_card(&ctx), get_tweak_player(&ctx)] {
      let addr = inst.expect("deref").address;
      assert_eq!(
        addr & 0xFF00_0000,
        0x8000_0000,
        "pointer 0x{addr:08x} not in RAM"
      );
    }
  }
}
