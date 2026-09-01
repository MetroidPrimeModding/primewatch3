use crate::mem::game_memory::GameMemory;
use crate::structs::prime_structs::GameStructs;

/// Explicit traversal context for the live-handle layer (`GameInstance`).
///
/// The C++ side reaches the loaded schema registry and `GameMemory` as ambient
/// globals; the Rust port threads them as two borrows instead. `Ctx` bundles the
/// pair so a live traversal (`a.member(ctx, "b").member(ctx, "c").read_u32(ctx)`)
/// carries a single argument.
///
/// It is `Copy` (two shared refs), but signatures take `&Ctx` for consistency
/// with the rest of the codebase.
///
/// The pure definitions layer (`GameStruct` / `GameMember` / `GameEnum` /
/// `GameStructs`) keeps its `&GameStructs` params — it has no business borrowing
/// `GameMemory`. `GameInstance` methods pass `ctx.structs` down into it.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
  pub structs: &'a GameStructs,
  pub mem: &'a GameMemory,
}

impl<'a> Ctx<'a> {
  pub fn new(structs: &'a GameStructs, mem: &'a GameMemory) -> Self {
    Self { structs, mem }
  }
}
