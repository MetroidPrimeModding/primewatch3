//! Generic egui tree view over any [`GameInstance`].
//!
//! Ports the *generic* half of `../primewatch2/src/defs/GameObjectRenderers.cpp`
//! — the recursive primitive / enum / array / nested-struct / `rstl::vector`
//! walk plus the pure `fmt::format` string helpers. The special-type table
//! (`CVector3f` / `CQuaternion` / `CTransform` / `CMatrix4f` / `SObjectTag`) is
//! P7.2; this module only leaves the [`SPECIAL_TYPES`] const and a `TODO(P7.2)`
//! dispatch hook for it.
//!
//! No call site yet — Phase 9 wires this into the watch windows.
//!
//! ## Deviations from the C++
//! - Reads are fallible (`Option`, P4.2). This module is the callsite that
//!   finally applies the C++ total-read default (`getRealPtr` clamps OOB to 0):
//!   `unwrap_or(0)` / `unwrap_or_default()`. "Unreadable" and "zero" render
//!   identically, exactly as they did in C++.
//! - The top-level `derefPointer` branch of C++ `render` is dropped: Rust
//!   instances arrive already-deref'd from [`GameInstance::get_member`] /
//!   `globals.rs`, and the carried [`GameInstance::pointer`] bit is used only for
//!   display (`*` prefix, null check, `u8*` C-string), never to re-deref.
//! - Click-to-copy copies the rendered label text. C++ sometimes copied a
//!   slightly different `clip` string (e.g. decimal-only for integers, raw hex
//!   bits for floats); that nicety is not reproduced.
//! - `CollapsingHeader` ids are salted with `(name, address)` rather than
//!   address alone, so sibling `extends` bases (same address, different name)
//!   don't collide. C++ keyed its `###` id on `name + offset`.
//! - `ARRAY_CAP` bounds the array loop; C++ has no failsafe against a corrupt
//!   `array_length` from a bad `.bs`.

use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;

/// Primitive leaf types — C++ `specialRenderers` maps each of these to
/// `primitiveRenderer` (`GameObjectRenderers.cpp:15-25`).
const PRIMITIVE_TYPES: &[&str] = &[
  "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "bool",
];

/// Types with a bespoke renderer in C++ `specialRenderers`
/// (`GameObjectRenderers.cpp:26-30`). Ported in P7.2; until then the dispatch
/// falls through to the generic struct walk.
pub const SPECIAL_TYPES: &[&str] = &[
  "CVector3f",
  "CQuaternion",
  "CTransform",
  "CMatrix4f",
  "SObjectTag",
];

/// Upper bound on rendered array elements per frame (C++ has none).
const ARRAY_CAP: u32 = 4096;

/// Port of the file-scope `bool GameObjectRenderers::render_exact_values`
/// (`GameObjectRenderers.cpp:12`). The "Show exact floating point values" menu
/// toggle that flips it is P9 (`PrimeWatch.cpp:476`).
pub struct Inspector {
  pub exact_values: bool,
}

impl Inspector {
  pub fn new() -> Self {
    Self {
      exact_values: false,
    }
  }
}

impl Default for Inspector {
  fn default() -> Self {
    Self::new()
  }
}

// ---------------------------------------------------------------------------
// Pure formatting helpers (no egui — unit-testable against the C++ fmt strings)
// ---------------------------------------------------------------------------

/// `{:#x}` of a negative integer the way C++ `fmt`/`std::format` does it:
/// sign-magnitude (`-0x8000`), not Rust's two's-complement (`0xffff…8000`).
fn c_hex_i64(v: i64) -> String {
  if v < 0 {
    format!("-{:#x}", v.unsigned_abs())
  } else {
    format!("{v:#x}")
  }
}

/// Ports `primitiveRenderer` (`GameObjectRenderers.cpp:165-245`). `name` already
/// includes any `*` pointer prefix. A failed read substitutes the C++
/// total-read default here (`0` / `0.0` / `""` / `false`).
pub fn format_primitive(ctx: &Ctx, name: &str, inst: &GameInstance, exact: bool) -> String {
  let typ = inst.type_name.as_str();
  match typ {
    // `u8` + pointer -> NUL-terminated C string.
    "u8" if inst.pointer => {
      let val = inst.read_string(ctx).unwrap_or_default();
      format!("{name} \"{val}\"")
    }
    "bool" => {
      let v = inst.read_bool(ctx).unwrap_or(false);
      format!("{name} {v}")
    }
    // Signed: no `GameInstance::read_i*` exists (P4.2) — read unsigned through
    // the bitfield path, then sign-extend. `i64` / unknown widths fall to the
    // `read_u64` branch, matching the C++ `else`.
    "i8" => {
      let v = inst.read_u8(ctx).unwrap_or(0) as i8 as i64;
      format!("{name} {v}/{}", c_hex_i64(v))
    }
    "i16" => {
      let v = inst.read_u16(ctx).unwrap_or(0) as i16 as i64;
      format!("{name} {v}/{}", c_hex_i64(v))
    }
    "i32" => {
      let v = inst.read_u32(ctx).unwrap_or(0) as i32 as i64;
      format!("{name} {v}/{}", c_hex_i64(v))
    }
    "i64" => {
      let v = inst.read_u64(ctx).unwrap_or(0) as i64;
      format!("{name} {v}/{}", c_hex_i64(v))
    }
    "u8" => {
      let v = inst.read_u8(ctx).unwrap_or(0) as u64;
      format!("{name} {v}/{v:#x}")
    }
    "u16" => {
      let v = inst.read_u16(ctx).unwrap_or(0) as u64;
      format!("{name} {v}/{v:#x}")
    }
    "u32" => {
      let v = inst.read_u32(ctx).unwrap_or(0) as u64;
      format!("{name} {v}/{v:#x}")
    }
    "u64" => {
      let v = inst.read_u64(ctx).unwrap_or(0);
      format!("{name} {v}/{v:#x}")
    }
    "f32" => {
      let f = inst.read_f32(ctx).unwrap_or(0.0);
      if exact {
        format!("{name} {f:.8}")
      } else {
        format!("{name} {f:.3}")
      }
    }
    "f64" => {
      let d = inst.read_f64(ctx).unwrap_or(0.0);
      if exact {
        format!("{name} {d:.16}")
      } else {
        format!("{name} {d:.3}")
      }
    }
    _ => format!("{name} Unknown number type {typ}"),
  }
}

/// Ports `renderEnum` (`GameObjectRenderers.cpp:83-103`). Unknown enum type ->
/// `"Unknown enum {type}"`; unknown value -> `"unknown"`.
pub fn format_enum(ctx: &Ctx, name: &str, inst: &GameInstance) -> String {
  let Some(game_enum) = ctx.structs.get_enum_by_name(&inst.type_name) else {
    return format!("Unknown enum {}", inst.type_name);
  };
  let value = inst.read_u32(ctx).unwrap_or(0);
  let ename = game_enum
    .get_name_by_value(value as i64)
    .unwrap_or_else(|| "unknown".to_string());
  format!("{name} {ename} ({value}/{value:#x}/{value:#b})")
}

/// Ports `hoverTooltip` (`GameObjectRenderers.cpp:63-81`): optional `*`, the
/// type name, optional `[{array_length}]`, ` {address:#08x}`, and an optional
/// `[bit {bit}; len {bit_length}]` when either is set. (C++ never uses the
/// member name here, so the port drops that parameter.)
pub fn hover_text(inst: &GameInstance) -> String {
  use std::fmt::Write as _;
  let mut msg = String::new();
  if inst.pointer {
    msg.push('*');
  }
  msg.push_str(&inst.type_name);
  if let Some(len) = inst.array_length {
    let _ = write!(msg, "[{len}]");
  }
  let _ = write!(msg, " {:#08x}", inst.address);
  if inst.bit.is_some() || inst.bit_length.is_some() {
    let bit = inst.bit.unwrap_or(0).max(0);
    let length = inst.bit_length.unwrap_or(0).max(0);
    let _ = write!(msg, "[bit {bit}; len {length}]");
  }
  msg
}

// ---------------------------------------------------------------------------
// egui walk
// ---------------------------------------------------------------------------

/// A clickable label that copies its own text to the clipboard on click
/// (C++ `ImGui::Text` + `IsItemClicked` -> `SetClipboardText`).
fn copyable_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
  let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
  if resp.clicked() {
    ui.ctx().copy_text(text.to_owned());
  }
  resp
}

impl Inspector {
  /// Ports `render` (`GameObjectRenderers.cpp:33-61`) minus the top-level
  /// pointer-deref branch (done upstream in [`GameInstance::get_member`]).
  /// Dispatch order: array -> primitive/special -> `rstl::vector` -> enum/struct.
  pub fn render(
    &self,
    ui: &mut egui::Ui,
    ctx: &Ctx,
    name: &str,
    inst: &GameInstance,
    add_tree: bool,
  ) {
    if inst.array_length.is_some() {
      self.render_array(ui, ctx, name, inst);
      return;
    }

    let typ = inst.type_name.as_str();

    if PRIMITIVE_TYPES.contains(&typ) {
      let text = format_primitive(ctx, name, inst, self.exact_values);
      copyable_label(ui, &text).on_hover_text(hover_text(inst));
      return;
    }

    if SPECIAL_TYPES.contains(&typ) {
      // TODO(P7.2): dispatch to the special-type renderers (CVector3f /
      // CQuaternion / CTransform / CMatrix4f / SObjectTag). Until P7.2 lands,
      // fall through to the generic enum/struct walk.
    }

    if typ.starts_with("rstl::vector<") || typ.starts_with("rstl::vector2<") {
      self.render_vector(ui, ctx, name, inst);
      return;
    }

    self.render_enum_or_struct(ui, ctx, name, inst, add_tree);
  }

  /// Ports `renderEnumOrStruct` (`GameObjectRenderers.cpp:105-155`).
  fn render_enum_or_struct(
    &self,
    ui: &mut egui::Ui,
    ctx: &Ctx,
    name: &str,
    inst: &GameInstance,
    add_tree: bool,
  ) {
    if ctx.structs.get_enum_by_name(&inst.type_name).is_some() {
      let text = format_enum(ctx, name, inst);
      copyable_label(ui, &text).on_hover_text(hover_text(inst));
      return;
    }

    let Some(game_struct) = ctx.structs.get_struct_by_name(&inst.type_name) else {
      ui.label(format!("Unknown type {}", inst.type_name))
        .on_hover_text(hover_text(inst));
      return;
    };

    if add_tree {
      let resp = egui::CollapsingHeader::new(name)
        .id_salt((name, inst.address))
        .show(ui, |ui| {
          self.render_struct_body(ui, ctx, inst, &game_struct);
        });
      resp.header_response.on_hover_text(hover_text(inst));
    } else {
      self.render_struct_body(ui, ctx, inst, &game_struct);
    }
  }

  /// The body of a struct subtree (`GameObjectRenderers.cpp:127-150`): "null" for
  /// a zero address, otherwise each `extends` base as its own subtree followed by
  /// every member in offset order.
  ///
  /// Deviation: C++ iterates `members_by_order` (declaration order); the Rust
  /// `GameStruct` only keeps `members_by_offset`. Offset order matches
  /// declaration order for every well-formed `.bs`.
  fn render_struct_body(
    &self,
    ui: &mut egui::Ui,
    ctx: &Ctx,
    inst: &GameInstance,
    game_struct: &crate::structs::prime_structs::GameStruct,
  ) {
    if inst.address == 0 {
      ui.label("null");
      return;
    }

    for parent in &game_struct.extends {
      let base = GameInstance::new(inst.address, parent.clone());
      self.render(ui, ctx, parent, &base, true);
    }

    for member in game_struct.members_by_offset.values() {
      let Some(child) = inst.get_member(ctx, &member.name) else {
        continue;
      };
      let child_name = if member.pointer {
        format!("*{}", member.name)
      } else {
        member.name.clone()
      };
      self.render(ui, ctx, &child_name, &child, true);
    }
  }

  /// Ports `renderArray` (`GameObjectRenderers.cpp:425-451`). `element` already
  /// strides by `element_size` and clears `array_length` on the child (P4.3).
  fn render_array(&self, ui: &mut egui::Ui, ctx: &Ctx, name: &str, inst: &GameInstance) {
    let count = inst.array_length.unwrap_or(0).max(0) as u32;
    let resp = egui::CollapsingHeader::new(name)
      .id_salt((name, inst.address))
      .show(ui, |ui| {
        for i in 0..count.min(ARRAY_CAP) {
          let element = inst.element(ctx, i);
          self.render(ui, ctx, &i.to_string(), &element, true);
        }
      });
    resp.header_response.on_hover_text(hover_text(inst));
  }

  /// Ports `renderVector` (`GameObjectRenderers.cpp:377-423`). `end` is the live
  /// count, `size` the capacity (rstl naming is inverted vs `std::vector` — the
  /// C++ label text `size: {end} max size: {size}` is kept verbatim). A
  /// per-vector `InputInt` index selects the one element rendered.
  fn render_vector(&self, ui: &mut egui::Ui, ctx: &Ctx, name: &str, inst: &GameInstance) {
    let resp = egui::CollapsingHeader::new(name)
      .id_salt((name, inst.address))
      .show(ui, |ui| {
        let end = inst.member(ctx, "end").read_u32(ctx).unwrap_or(0);
        let size = inst.member(ctx, "size").read_u32(ctx).unwrap_or(0);
        ui.label(format!("size: {end} max size: {size}"));

        let id = ui.make_persistent_id((inst.address, "vec_index"));
        let mut index: i32 = ui.ctx().data_mut(|d| d.get_temp(id).unwrap_or(0));
        // Clamp to a sane i32; a garbage `end` must not make `min > max` (panics).
        let max_index = end.saturating_sub(1).min(i32::MAX as u32) as i32;
        ui.add(egui::DragValue::new(&mut index).range(0..=max_index));
        index = index.clamp(0, max_index);
        ui.ctx().data_mut(|d| d.insert_temp(id, index));

        // `first` auto-derefs to element 0; stride by the element type's size.
        let first = inst.member(ctx, "first");
        let element = first.element(ctx, index as u32);
        self.render(ui, ctx, &index.to_string(), &element, false);
      });
    resp.header_response.on_hover_text(hover_text(inst));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::{GameEnum, GameMember, GameStruct, GameStructs};
  use bimap::BiBTreeMap;
  use std::collections::BTreeMap;

  fn mem_with(writes: &[(usize, &[u8])]) -> GameMemory {
    let mut mem = GameMemory::new();
    for (offset, bytes) in writes {
      mem.data[*offset..*offset + bytes.len()].copy_from_slice(bytes);
    }
    mem
  }

  fn empty_struct(name: &str) -> GameStruct {
    GameStruct {
      name: name.to_string(),
      size: 0,
      vtable_address: None,
      extends: vec![],
      members_by_offset: BTreeMap::new(),
      members_by_name: BTreeMap::new(),
    }
  }

  #[test]
  fn format_primitive_u32() {
    let structs = GameStructs::new_empty();
    let mem = mem_with(&[(0, &0xDEAD_BEEFu32.to_be_bytes())]);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(0x8000_0000, "u32".to_string());
    assert_eq!(
      format_primitive(&ctx, "field", &inst, false),
      "field 3735928559/0xdeadbeef"
    );
  }

  #[test]
  fn format_primitive_f32_exact_and_not() {
    let structs = GameStructs::new_empty();
    let mem = mem_with(&[(0, &1.5f32.to_be_bytes())]);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(0x8000_0000, "f32".to_string());
    assert_eq!(format_primitive(&ctx, "x", &inst, false), "x 1.500");
    assert_eq!(format_primitive(&ctx, "x", &inst, true), "x 1.50000000");
  }

  #[test]
  fn format_primitive_bool() {
    let structs = GameStructs::new_empty();
    let mem = mem_with(&[(0, &[1]), (1, &[0])]);
    let ctx = Ctx::new(&structs, &mem);
    let t = GameInstance::new(0x8000_0000, "bool".to_string());
    let f = GameInstance::new(0x8000_0001, "bool".to_string());
    assert_eq!(format_primitive(&ctx, "b", &t, false), "b true");
    assert_eq!(format_primitive(&ctx, "b", &f, false), "b false");
  }

  #[test]
  fn format_primitive_i16_sign_extends() {
    let structs = GameStructs::new_empty();
    let mem = mem_with(&[(0, &0x8000u16.to_be_bytes())]);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(0x8000_0000, "i16".to_string());
    // exact flag is irrelevant for integers.
    assert_eq!(
      format_primitive(&ctx, "s", &inst, false),
      "s -32768/-0x8000"
    );
    assert_eq!(format_primitive(&ctx, "s", &inst, true), "s -32768/-0x8000");
  }

  #[test]
  fn format_primitive_u8_pointer_is_cstring() {
    let structs = GameStructs::new_empty();
    let mem = mem_with(&[(0x10, b"hi\0")]);
    let ctx = Ctx::new(&structs, &mem);
    let mut inst = GameInstance::new(0x8000_0010, "u8".to_string());
    inst.pointer = true;
    assert_eq!(
      format_primitive(&ctx, "*name", &inst, false),
      "*name \"hi\""
    );
  }

  fn direction_enum() -> GameEnum {
    let mut values = BiBTreeMap::new();
    values.insert("kNone".to_string(), 0i64);
    values.insert("kTwo".to_string(), 2i64);
    GameEnum {
      name: "EDir".to_string(),
      size: 4,
      values,
    }
  }

  #[test]
  fn format_enum_known_and_unknown_value() {
    let mut structs = GameStructs::new_empty();
    structs.insert_enum(&direction_enum());
    let mem = mem_with(&[(0, &2u32.to_be_bytes()), (4, &7u32.to_be_bytes())]);
    let ctx = Ctx::new(&structs, &mem);

    let known = GameInstance::new(0x8000_0000, "EDir".to_string());
    assert_eq!(format_enum(&ctx, "dir", &known), "dir kTwo (2/0x2/0b10)");

    let unknown_val = GameInstance::new(0x8000_0004, "EDir".to_string());
    assert_eq!(
      format_enum(&ctx, "dir", &unknown_val),
      "dir unknown (7/0x7/0b111)"
    );

    let unknown_enum = GameInstance::new(0x8000_0000, "ENope".to_string());
    assert_eq!(
      format_enum(&ctx, "dir", &unknown_enum),
      "Unknown enum ENope"
    );
  }

  #[test]
  fn hover_text_variants() {
    // Plain struct instance.
    let plain = GameInstance::new(0x8000_0010, "CFoo".to_string());
    assert_eq!(hover_text(&plain), "CFoo 0x80000010");

    // Bitfield.
    let bf = GameInstance::with_bitfield(0x8000_0020, "u32".to_string(), Some(3), Some(5));
    assert_eq!(hover_text(&bf), "u32 0x80000020[bit 3; len 5]");

    // Array + pointer, carried through a resolved member.
    let mut structs = GameStructs::new_empty();
    let arr = GameMember {
      type_name: "u8".to_string(),
      name: "buf".to_string(),
      offset: 0,
      bit: None,
      bit_length: None,
      array_length: Some(4),
      pointer: true,
    };
    let mut owner = empty_struct("Owner");
    owner.insert_member(&arr);
    structs.insert_struct(&owner);
    let mem = mem_with(&[(0, &0x8000_1000u32.to_be_bytes())]);
    let ctx = Ctx::new(&structs, &mem);
    let child = GameInstance::new(0x8000_0000, "Owner".to_string())
      .get_member(&ctx, "buf")
      .unwrap();
    assert_eq!(hover_text(&child), "*u8[4] 0x80001000");
  }
}
