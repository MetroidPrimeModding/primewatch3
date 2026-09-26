//! Which Metroid Prime revision is running (see `doc/multi-version.md`).
//!
//! Detected from the disc header the apploader copies to `0x80000000`: the 6-byte
//! game ID followed by the disc number and the disc revision byte.

use crate::mem::game_memory::GameMemory;
use std::path::Path;

/// Every revision `prime-decomp` knows about, in its `VERSIONS` order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum GameVersion {
  /// The revision the `.bs` literal offsets were written against.
  #[default]
  NtscU0_00,
  NtscU0_01,
  NtscK,
  Pal,
  NtscJ,
  NtscU0_02,
  NewPlayControlJ,
  TrilogyNtsc,
  TrilogyPal,
}

const GAME_ID_ADDR: u32 = 0x8000_0000;
const DISC_REVISION_ADDR: u32 = 0x8000_0007;

impl GameVersion {
  pub const ALL: [GameVersion; 9] = [
    GameVersion::NtscU0_00,
    GameVersion::NtscU0_01,
    GameVersion::NtscK,
    GameVersion::Pal,
    GameVersion::NtscJ,
    GameVersion::NtscU0_02,
    GameVersion::NewPlayControlJ,
    GameVersion::TrilogyNtsc,
    GameVersion::TrilogyPal,
  ];

  /// The decomp's name for this revision (`config/<id>/`, `prime_defs/layouts/<id>.json.gz`).
  pub fn id(self) -> &'static str {
    match self {
      GameVersion::NtscU0_00 => "GM8E01_00",
      GameVersion::NtscU0_01 => "GM8E01_01",
      GameVersion::NtscK => "GM8E01_48",
      GameVersion::Pal => "GM8P01_00",
      GameVersion::NtscJ => "GM8J01_00",
      GameVersion::NtscU0_02 => "GM8E01_02",
      GameVersion::NewPlayControlJ => "R3IJ01_00",
      GameVersion::TrilogyNtsc => "R3ME01_00",
      GameVersion::TrilogyPal => "R3MP01_00",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      GameVersion::NtscU0_00 => "NTSC-U 0-00",
      GameVersion::NtscU0_01 => "NTSC-U 0-01",
      GameVersion::NtscK => "NTSC-K",
      GameVersion::Pal => "PAL",
      GameVersion::NtscJ => "NTSC-J",
      GameVersion::NtscU0_02 => "NTSC-U 0-02",
      GameVersion::NewPlayControlJ => "New Play Control (JP)",
      GameVersion::TrilogyNtsc => "Trilogy NTSC",
      GameVersion::TrilogyPal => "Trilogy PAL",
    }
  }

  /// Whether `defs_dir` has this revision's own decomp layouts, so `.bs` `= field`
  /// members resolve for it. Members with literal offsets are GM8E01_00's regardless.
  pub fn is_supported(self, defs_dir: &Path) -> bool {
    self.layout_version(defs_dir) == self
  }

  /// The Korean release shares the NTSC-U game ID and is told apart only by its
  /// revision byte (48).
  ///
  /// Gotcha: Trilogy boots Prime 1, 2 and 3 from the same disc, so an `R3M*` ID
  /// doesn't prove Prime 1 is the game running.
  pub fn from_disc_header(game_id: &[u8], revision: u8) -> Option<GameVersion> {
    Some(match (game_id, revision) {
      (b"GM8E01", 0) => GameVersion::NtscU0_00,
      (b"GM8E01", 1) => GameVersion::NtscU0_01,
      (b"GM8E01", 2) => GameVersion::NtscU0_02,
      (b"GM8E01", 48) => GameVersion::NtscK,
      (b"GM8P01", 0) => GameVersion::Pal,
      (b"GM8J01", 0) => GameVersion::NtscJ,
      (b"R3IJ01", 0) => GameVersion::NewPlayControlJ,
      (b"R3ME01", 0) => GameVersion::TrilogyNtsc,
      (b"R3MP01", 0) => GameVersion::TrilogyPal,
      _ => return None,
    })
  }

  /// `None` when no game is loaded (zeroed header) or it isn't a known Prime revision.
  pub fn detect(mem: &GameMemory) -> Option<GameVersion> {
    let mut game_id = [0u8; 6];
    for (i, byte) in game_id.iter_mut().enumerate() {
      *byte = mem.read_u8(GAME_ID_ADDR + i as u32)?;
    }
    GameVersion::from_disc_header(&game_id, mem.read_u8(DISC_REVISION_ADDR)?)
  }
}

impl std::fmt::Display for GameVersion {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{} ({})", self.label(), self.id())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ids_are_unique_and_decomp_shaped() {
    for (i, a) in GameVersion::ALL.iter().enumerate() {
      assert_eq!(a.id().len(), 9, "{a:?}");
      for b in &GameVersion::ALL[i + 1..] {
        assert_ne!(a.id(), b.id());
      }
    }
  }

  #[test]
  fn header_round_trips_through_id() {
    for v in GameVersion::ALL {
      let (game_id, rev) = v.id().split_once('_').unwrap();
      let rev: u8 = rev.parse().unwrap();
      assert_eq!(
        GameVersion::from_disc_header(game_id.as_bytes(), rev),
        Some(v)
      );
    }
  }

  #[test]
  fn detects_from_memory() {
    let mut mem = GameMemory::new();
    assert_eq!(GameVersion::detect(&mem), None);

    mem.data[..8].copy_from_slice(b"GM8E01\0\x30");
    assert_eq!(GameVersion::detect(&mem), Some(GameVersion::NtscK));

    mem.data[..8].copy_from_slice(b"GM8P01\0\0");
    assert_eq!(GameVersion::detect(&mem), Some(GameVersion::Pal));

    // Prime 2
    mem.data[..8].copy_from_slice(b"G2ME01\0\0");
    assert_eq!(GameVersion::detect(&mem), None);
  }
}
