//! The fixed global roots the app traverses from every frame.
//!
//! Each root's address doubles as its live address. The non-pointer roots
//! (`g_stateManager`, `g_main`) map straight to a `GameInstance` at that
//! address. The pointer roots (`gp_MemoryCard`, `gp_TweakPlayer`) hold a `u32`
//! pointer *at* the fixed address; here that deref is explicit and can fail
//! (unreadable memory / null), so those return `Option<GameInstance>`.

use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;
use std::string::ToString;

pub fn get_state_manager() -> GameInstance {
  GameInstance::new(0x8045A1A8, "CStateManager".to_string())
}

pub fn get_main() -> GameInstance {
  GameInstance::new(0x80457560, "CMain".to_string())
}

/// `gp_MemoryCard` — pointer at `0x805A8C44` to a `CMemoryCardSys`.
pub fn get_memory_card(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.mem.read_u32(0x805A8C44)?;
  Some(GameInstance::new(address, "CMemoryCardSys".to_string()))
}

/// `gp_TweakPlayer` — pointer at `0x805A8CD8` to a `CTweakPlayer`.
pub fn get_tweak_player(ctx: &Ctx) -> Option<GameInstance> {
  let address = ctx.mem.read_u32(0x805A8CD8)?;
  Some(GameInstance::new(address, "CTweakPlayer".to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::GameStructs;

  #[test]
  fn non_pointer_roots_match_game_offsets_hpp() {
    let sm = get_state_manager();
    assert_eq!(sm.address, 0x8045A1A8);
    assert_eq!(sm.type_name, "CStateManager");
    let main = get_main();
    assert_eq!(main.address, 0x80457560);
    assert_eq!(main.type_name, "CMain");
  }

  #[test]
  fn pointer_roots_deref_the_fixed_address() {
    // Zeroed memory: the pointer slots read back 0, so the deref succeeds with
    // address 0 (it does not fail — only an OOB / unreadable slot yields None).
    let structs = GameStructs::new_empty();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);

    let card = get_memory_card(&ctx).unwrap();
    assert_eq!(card.address, mem.read_u32(0x805A8C44).unwrap());
    assert_eq!(card.type_name, "CMemoryCardSys");

    let tweak = get_tweak_player(&ctx).unwrap();
    assert_eq!(tweak.address, mem.read_u32(0x805A8CD8).unwrap());
    assert_eq!(tweak.type_name, "CTweakPlayer");
  }

  #[test]
  fn pointer_roots_deref_live_dump_when_present() {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping globals mem1.raw test: {path} not found");
      return;
    }
    let structs = GameStructs::new_empty();
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    let ctx = Ctx::new(&structs, &mem);

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
