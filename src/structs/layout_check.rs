//! Validates every `.bs` member against the decomp's DWARF layouts
//! (`prime_defs/layouts/GM8E01_00.json.gz`).
//!
//! `= field` members get their offset from DWARF, so this mostly checks that their
//! `.bs` type fits the field; literal offsets are checked for position too. Either way
//! each member is checked by walking the DWARF layout down to whatever sits at its offset:
//! - a scalar (`u32`, `f32`, pointer, enum) must land exactly on a DWARF scalar
//!   of the same size and a compatible kind (int / float / pointer);
//! - a bitfield must cover exactly the same bits as a DWARF bitfield, or sit
//!   inside a DWARF integer (flag words read bit-by-bit);
//! - a struct-typed member must land on a DWARF member of that type, or of a
//!   type deriving from it. Landing *inside* one is also accepted, because some
//!   `.bs` rstl templates deliberately point past a leading word (see `rstl.bs`);
//! - anything inside `uchar[]` storage (`reserved_vector`, `optional_object`) is
//!   accepted, since the storage is untyped.

use super::layouts::{BitData, LayoutDb, StructDef, TypeRef, short_name};
use super::prime_structs::{GameMember, GameStruct, GameStructs, primitive_size};
use crate::mem::game_version::GameVersion;
use std::fmt::Write;
use std::rc::Rc;

/// The decomp's unqualified name for `.bs` type `bs_name` (its `decomp` declaration).
fn decomp_short_name(structs: &GameStructs, bs_name: &str) -> Rc<str> {
  let decomp = structs
    .get_struct_by_name(bs_name)
    .map_or_else(|| bs_name.into(), |s| s.decomp_name.clone());
  short_name(&decomp).into()
}

#[derive(Clone)]
struct Frame {
  path: String,
  /// Absolute offset within the checked struct.
  start: u32,
  size: u32,
  ty: TypeRef,
  bit: Option<BitData>,
}

/// Every chain of nested DWARF members covering byte `target`, outermost first.
/// More than one when members overlap (unions, anonymous unions mwcc flattened,
/// bitfields sharing a storage unit).
fn chains(db: &LayoutDb, def: &StructDef, at: u32, target: u32, prefix: &str) -> Vec<Vec<Frame>> {
  let mut out = Vec::new();
  for base in &def.bases {
    let Some(base_def) = db.resolve_base(base) else {
      continue;
    };
    let start = at + base.offset;
    if (start..start + base_def.byte_size.unwrap_or(0)).contains(&target) {
      out.extend(chains(db, base_def, start, target, prefix));
    }
  }
  for m in &def.members {
    let start = at + m.offset;
    let size = m.byte_size.unwrap_or(0);
    if !(start..start + size).contains(&target) {
      continue;
    }
    let name = m.name.as_deref().unwrap_or("<anon>");
    let path = if prefix.is_empty() {
      name.to_string()
    } else {
      format!("{prefix}.{name}")
    };
    let frame = Frame {
      path: path.clone(),
      start,
      size,
      ty: m.ty.clone(),
      bit: m.bit,
    };
    for mut tail in descend(db, &m.ty, start, size, target, &path) {
      tail.insert(0, frame.clone());
      out.push(tail);
    }
  }
  out
}

fn descend(
  db: &LayoutDb,
  ty: &TypeRef,
  start: u32,
  size: u32,
  target: u32,
  path: &str,
) -> Vec<Vec<Frame>> {
  if ty.is_array() {
    let count: u32 = ty.array_dims.iter().product();
    if count == 0 || size < count {
      return vec![vec![]];
    }
    let elem_size = size / count;
    let index = (target - start) / elem_size;
    let elem = Frame {
      path: format!("{path}[{index}]"),
      start: start + index * elem_size,
      size: elem_size,
      ty: ty.element(),
      bit: None,
    };
    let mut tails = descend(db, &elem.ty, elem.start, elem_size, target, &elem.path);
    for tail in &mut tails {
      tail.insert(0, elem.clone());
    }
    return tails;
  }
  if ty.is_aggregate()
    && let Some(def) = db.resolve(ty)
  {
    let inner = chains(db, &def, start, target, path);
    if !inner.is_empty() {
      return inner;
    }
  }
  vec![vec![]]
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
  Int,
  Float,
  Pointer,
}

fn leaf_kind(ty: &TypeRef) -> Option<Kind> {
  if ty.is_pointer() {
    return Some(Kind::Pointer);
  }
  if ty.is_array() || ty.is_aggregate() {
    return None;
  }
  match ty.base.as_str() {
    "float" | "double" => Some(Kind::Float),
    _ => Some(Kind::Int),
  }
}

fn compatible(want: Kind, have: Kind) -> bool {
  // `u32` reads of pointers (`u32 vtable 0x0`, handles stored as ids) are fine.
  matches!(
    (want, have),
    (Kind::Int, Kind::Int)
      | (Kind::Int, Kind::Pointer)
      | (Kind::Float, Kind::Float)
      | (Kind::Pointer, Kind::Pointer)
  )
}

/// Byte-sized element of an array: untyped inline storage.
fn in_byte_storage(chain: &[Frame]) -> bool {
  let n = chain.len();
  n >= 2
    && chain[n - 2].ty.is_array()
    && chain[n - 1].size == 1
    && leaf_kind(&chain[n - 1].ty) == Some(Kind::Int)
}

enum Outcome {
  Exact,
  Interior,
  ByteStorage,
  /// A different type, but every member of the `.bs` struct checks out.
  Structural,
  /// Lands on an aggregate dtk has no definition for; can't be verified.
  Opaque,
  Mismatch,
}

fn check_scalar(chains: &[Vec<Frame>], at: u32, want: Kind, size: u32) -> Outcome {
  let mut outcome = Outcome::Mismatch;
  for chain in chains {
    let Some(last) = chain.last() else { continue };
    if last.bit.is_some() {
      if want == Kind::Int && last.start == at {
        if last.size == size {
          return Outcome::Exact;
        }
        // A flags word read whole, over bitfields mwcc packed into smaller units.
        if size > last.size {
          outcome = Outcome::Interior;
        }
      }
      continue;
    }
    if let Some(have) = leaf_kind(&last.ty) {
      if last.start == at && last.size == size && compatible(want, have) {
        return Outcome::Exact;
      }
      // Part of a wider integer, e.g. the two u32 halves of a u64 bitmask.
      if want == Kind::Int && have == Kind::Int && at + size <= last.start + last.size {
        outcome = Outcome::Interior;
      } else if in_byte_storage(chain) {
        outcome = Outcome::ByteStorage;
      }
    } else if last.start == at {
      outcome = Outcome::Opaque;
    }
  }
  outcome
}

fn check_bits(chains: &[Vec<Frame>], start_bit: u32, len: u32) -> Outcome {
  let mut outcome = Outcome::Mismatch;
  for chain in chains {
    let Some(last) = chain.last() else { continue };
    let field_start = last.start * 8;
    match last.bit {
      Some(bit) => {
        if field_start + bit.bit_offset == start_bit && bit.bit_size == len {
          return Outcome::Exact;
        }
      }
      None => {
        let field_end = field_start + last.size * 8;
        if leaf_kind(&last.ty) == Some(Kind::Int)
          && start_bit >= field_start
          && start_bit + len <= field_end
        {
          outcome = Outcome::Interior;
        }
      }
    }
  }
  outcome
}

/// Whether `def`, or anything it derives from, is named `want` (a decomp short name).
fn derives_from(db: &LayoutDb, def: &StructDef, want: &str) -> bool {
  short_name(&def.name) == want
    || def.bases.iter().any(|b| {
      short_name(&b.name) == want
        || db
          .resolve_base(b)
          .is_some_and(|d| derives_from(db, d, want))
    })
}

fn type_matches(db: &LayoutDb, ty: &TypeRef, want: &str) -> bool {
  short_name(&ty.base) == want || db.resolve(ty).is_some_and(|d| derives_from(db, &d, want))
}

fn check_struct(db: &LayoutDb, chains: &[Vec<Frame>], at: u32, want: &str) -> Outcome {
  let mut outcome = Outcome::Mismatch;
  for chain in chains {
    for frame in chain {
      if frame.ty.is_aggregate() && type_matches(db, &frame.ty, want) {
        if frame.start == at {
          return Outcome::Exact;
        }
        outcome = Outcome::Interior;
      }
    }
    if matches!(outcome, Outcome::Mismatch) && in_byte_storage(chain) {
      outcome = Outcome::ByteStorage;
    }
  }
  outcome
}

fn describe(chains: &[Vec<Frame>]) -> String {
  let found: Vec<String> = chains
    .iter()
    .filter_map(|c| c.last())
    .map(|f| {
      let bits = f
        .bit
        .map(|b| format!(" bits {}+{}", b.bit_offset, b.bit_size))
        .unwrap_or_default();
      format!(
        "{} @{:#x} size {} `{}`{bits}",
        f.path, f.start, f.size, f.ty.display
      )
    })
    .collect();
  if found.is_empty() {
    "nothing (padding, or past the end)".to_string()
  } else {
    found.join(" | ")
  }
}

#[derive(Default)]
struct Report {
  structs_checked: usize,
  members_checked: usize,
  exact: usize,
  interior: usize,
  byte_storage: usize,
  structural: usize,
  opaque: Vec<String>,
  mismatches: Vec<String>,
  unmatched_structs: Vec<String>,
}

fn member_size(structs: &GameStructs, m: &GameMember) -> Option<u32> {
  if m.pointer {
    Some(4)
  } else if let Some(e) = structs.get_enum_by_name(&m.type_name) {
    Some(e.size as u32)
  } else if let Some(s) = structs.get_struct_by_name(&m.type_name) {
    Some(s.size as u32)
  } else {
    Some(primitive_size(&m.type_name))
  }
}

/// Checks `m`, a member of a `.bs` struct placed at `base` within DWARF `def`.
fn check_member(
  db: &LayoutDb,
  structs: &GameStructs,
  def: &StructDef,
  base: u32,
  m: &GameMember,
) -> (Outcome, String) {
  let at = base + m.offset as u32;
  if let (Some(bit), Some(len)) = (m.bit, m.bit_length)
    && len > 0
  {
    let width_bits = primitive_size(&m.type_name) * 8;
    let start_bit = at * 8 + width_bits - bit as u32 - len as u32;
    let found = chains(db, def, 0, start_bit / 8, "");
    return (check_bits(&found, start_bit, len as u32), describe(&found));
  }

  let bs_struct = (!m.pointer)
    .then(|| structs.get_struct_by_name(&m.type_name))
    .flatten();
  let kind = if m.pointer {
    Kind::Pointer
  } else if matches!(m.type_name.as_ref(), "f32" | "f64") {
    Kind::Float
  } else {
    Kind::Int
  };
  let size = member_size(structs, m).unwrap_or(4);
  let check_at = |at: u32| {
    let found = chains(db, def, 0, at, "");
    let Some(bs_struct) = &bs_struct else {
      return (check_scalar(&found, at, kind, size), describe(&found));
    };
    let outcome = check_struct(db, &found, at, &decomp_short_name(structs, &m.type_name));
    if !matches!(outcome, Outcome::Mismatch) || bs_struct.members_by_order.is_empty() {
      return (outcome, describe(&found));
    }
    // Not the same type, but maybe the same shape: `CTransform` rows declared as
    // `CVector3f` where the decomp has twelve floats.
    for inner in &bs_struct.members_by_order {
      let (outcome, found) = check_member(db, structs, def, at, inner);
      if matches!(outcome, Outcome::Mismatch) {
        return (
          outcome,
          format!(
            "{found} (checking `.{}` of `{}`)",
            inner.name, bs_struct.name
          ),
        );
      }
    }
    (Outcome::Structural, describe(&found))
  };

  let first = check_at(at);
  // For arrays, also check the last element so a too-long `.bs` array is caught.
  match m.array_length {
    Some(n) if n > 1 && !matches!(first.0, Outcome::Mismatch) => {
      check_at(at + (n as u32 - 1) * size)
    }
    _ => first,
  }
}

fn check_struct_def(db: &LayoutDb, structs: &GameStructs, bs: &GameStruct, report: &mut Report) {
  let def = match db.find_struct(&bs.decomp_name) {
    Ok(def) => def,
    Err(why) => {
      report
        .unmatched_structs
        .push(format!("{} ({why})", bs.name));
      return;
    }
  };
  report.structs_checked += 1;

  for parent in &bs.extends {
    if !derives_from(db, def, &decomp_short_name(structs, parent)) {
      report.mismatches.push(format!(
        "{}: extends {parent}, but DWARF {} doesn't derive from it",
        bs.name, def.name
      ));
    }
  }

  for m in &bs.members_by_order {
    report.members_checked += 1;
    let (mut outcome, found) = check_member(db, structs, def, 0, m);
    // A stub (TCastTo.cpp-style: bases and size, no members) can't disprove anything.
    if matches!(outcome, Outcome::Mismatch) && def.members.is_empty() {
      outcome = Outcome::Opaque;
    }
    let bits = match (m.bit, m.bit_length) {
      (Some(b), Some(l)) if l > 0 => format!(":{b}:{l}"),
      _ => String::new(),
    };
    let ptr = if m.pointer { "*" } else { "" };
    let what = format!(
      "{}.{} @{:#x}{bits} ({ptr}{})",
      bs.name, m.name, m.offset, m.type_name
    );
    match outcome {
      Outcome::Exact => report.exact += 1,
      Outcome::Interior => report.interior += 1,
      Outcome::ByteStorage => report.byte_storage += 1,
      Outcome::Structural => report.structural += 1,
      Outcome::Opaque => report.opaque.push(format!("{what}: DWARF has {found}")),
      Outcome::Mismatch => report.mismatches.push(format!("{what}: DWARF has {found}")),
    }
  }
}

fn run(db: &LayoutDb, structs: &GameStructs) -> Report {
  let mut report = Report::default();
  for bs in structs.structs.values() {
    // rstl templates are laid out relative to where `.bs` members point into them
    // (see rstl.bs), not like the decomp's classes; they're covered indirectly by
    // the struct-typed member checks instead.
    if bs.name.contains('<') || bs.name.starts_with("rstl::") {
      continue;
    }
    check_struct_def(db, structs, bs, &mut report);
  }
  report
}

fn format_report(r: &Report) -> String {
  let mut s = String::new();
  let _ = writeln!(
    s,
    "{} structs checked ({} without a DWARF match), {} members: {} exact, {} inside a matching member, {} structurally compatible, {} in byte storage, {} unverifiable, {} mismatched",
    r.structs_checked,
    r.unmatched_structs.len(),
    r.members_checked,
    r.exact,
    r.interior,
    r.structural,
    r.byte_storage,
    r.opaque.len(),
    r.mismatches.len()
  );
  for (title, list) in [
    ("Mismatches", &r.mismatches),
    ("Unverifiable (no DWARF definition)", &r.opaque),
    ("Structs without a DWARF match", &r.unmatched_structs),
  ] {
    if !list.is_empty() {
      let _ = writeln!(s, "\n{title}:");
      for line in list {
        let _ = writeln!(s, "  {line}");
      }
    }
  }
  s
}

#[test]
fn bs_offsets_match_gm8e01_00_dwarf() {
  let db = GameVersion::NtscU0_00.layouts();
  let mut structs = GameStructs::new_empty();
  structs
    .load_from_dir(
      concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"),
      GameVersion::NtscU0_00,
    )
    .expect("load prime_defs");

  let report = run(db, &structs);
  let text = format_report(&report);
  println!("{text}");
  assert!(report.mismatches.is_empty(), "{text}");
}
