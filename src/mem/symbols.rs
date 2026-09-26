//! Per-revision symbol addresses, generated from prime-decomp's `symbols.txt` by
//! `tools/gen_decomp_data.sh` (see `doc/multi-version.md`) and compiled in.
//!
//! Only the matching decomp's addresses are used: a `-g` build moves code and data.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::mem::game_version::GameVersion;

pub struct SymbolTable {
  /// `None` for names the decomp gives to several objects (statics in different units).
  by_name: HashMap<&'static str, Option<u32>>,
  /// `__vt__<mangled class>` symbols, keyed by address, as demangled class names.
  vtable_classes: HashMap<u32, String>,
}

impl SymbolTable {
  fn parse(text: &'static str) -> SymbolTable {
    let mut by_name = HashMap::new();
    let mut vtable_classes = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
      let (addr, name) = line.split_once(' ').expect("`<address> <name>` line");
      let addr = u32::from_str_radix(addr.trim_start_matches("0x"), 16).expect("hex address");
      by_name
        .entry(name)
        .and_modify(|a| *a = None)
        .or_insert(Some(addr));
      if let Some(class) = name.strip_prefix("__vt__").and_then(demangle_class) {
        vtable_classes.insert(addr, class);
      }
    }
    SymbolTable {
      by_name,
      vtable_classes,
    }
  }

  pub fn address(&self, name: &str) -> Option<u32> {
    self.by_name.get(name).copied().flatten()
  }

  /// The class whose vtable starts at `address`. Nested classes are `Outer::Inner`.
  pub fn vtable_class(&self, address: u32) -> Option<&str> {
    self.vtable_classes.get(&address).map(String::as_str)
  }
}

/// CodeWarrior class-name mangling: `<len><name>`, or `Q<count>` followed by that many
/// `<len><name>` parts for a nested name. Template arguments stay mangled.
fn demangle_class(mangled: &str) -> Option<String> {
  fn part(s: &str) -> Option<(&str, &str)> {
    let digits = s.find(|c: char| !c.is_ascii_digit())?;
    let len: usize = s[..digits].parse().ok()?;
    let rest = &s[digits..];
    rest.get(..len).map(|name| (name, &rest[len..]))
  }

  let (count, mut rest) = match mangled.strip_prefix('Q') {
    Some(q) => (q.get(..1)?.parse().ok()?, &q[1..]),
    None => (1, mangled),
  };
  let mut parts = Vec::with_capacity(count);
  for _ in 0..count {
    let (name, tail) = part(rest)?;
    parts.push(name);
    rest = tail;
  }
  rest.is_empty().then(|| parts.join("::"))
}

fn source(version: GameVersion) -> Option<&'static str> {
  Some(match version {
    GameVersion::NtscU0_00 => include_str!("../../prime_defs/symbols/GM8E01_00.txt"),
    GameVersion::NtscU0_01 => include_str!("../../prime_defs/symbols/GM8E01_01.txt"),
    GameVersion::NtscK => include_str!("../../prime_defs/symbols/GM8E01_48.txt"),
    GameVersion::Pal => include_str!("../../prime_defs/symbols/GM8P01_00.txt"),
    GameVersion::NtscJ => include_str!("../../prime_defs/symbols/GM8J01_00.txt"),
    GameVersion::NtscU0_02 => include_str!("../../prime_defs/symbols/GM8E01_02.txt"),
    // The decomp doesn't have usable Wii symbols yet.
    GameVersion::NewPlayControlJ | GameVersion::TrilogyNtsc | GameVersion::TrilogyPal => {
      return None;
    }
  })
}

static TABLES: LazyLock<HashMap<GameVersion, SymbolTable>> = LazyLock::new(|| {
  GameVersion::ALL
    .into_iter()
    .filter_map(|v| Some((v, SymbolTable::parse(source(v)?))))
    .collect()
});

impl GameVersion {
  pub fn symbols(self) -> Option<&'static SymbolTable> {
    TABLES.get(&self)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn demangles_class_names() {
    assert_eq!(demangle_class("7CPlayer").as_deref(), Some("CPlayer"));
    assert_eq!(
      demangle_class("Q213CMetroidPrime14CMissileTarget").as_deref(),
      Some("CMetroidPrime::CMissileTarget")
    );
    assert_eq!(
      demangle_class("39TObjOwnerDerivedFromIObj<11CRasterFont>").as_deref(),
      Some("TObjOwnerDerivedFromIObj<11CRasterFont>")
    );
    assert_eq!(demangle_class("7CPlaye"), None);
    assert_eq!(demangle_class("7CPlayerX"), None);
  }

  #[test]
  fn every_gamecube_revision_has_the_roots() {
    for v in GameVersion::ALL {
      let Some(symbols) = v.symbols() else {
        continue;
      };
      for name in [
        "sMainSpace",
        "sAllocSpace$CStateManager",
        "gpMemoryCard",
        "gpTweakPlayer",
      ] {
        assert!(symbols.address(name).is_some(), "{v}: {name}");
      }
    }
    assert!(GameVersion::NtscU0_00.symbols().is_some());
  }

  /// Addresses PrimeWatch hardcoded before it read them from the decomp.
  #[test]
  fn gm8e01_00_matches_the_old_hardcoded_addresses() {
    let s = GameVersion::NtscU0_00.symbols().unwrap();
    assert_eq!(s.address("sMainSpace"), Some(0x80457560));
    assert_eq!(s.address("sAllocSpace$CStateManager"), Some(0x8045A1A8));
    assert_eq!(s.address("gpMemoryCard"), Some(0x805A8C44));
    assert_eq!(s.address("gpTweakPlayer"), Some(0x805A8CD8));
    assert_eq!(s.vtable_class(0x803D96E8), Some("CPlayer"));
    assert_eq!(s.vtable_class(0x803DF890), Some("CBeetle"));
    assert_eq!(s.vtable_class(0x803D9E30), Some("CEntity"));
    assert_eq!(s.vtable_class(0x803D96E9), None);
    assert_eq!(s.address("kGunLocator"), None, "ambiguous");
  }
}
