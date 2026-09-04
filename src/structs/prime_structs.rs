use crate::ctx::Ctx;
use bimap::BiBTreeMap;
use bstruct::bstruct_link::{BEnum, BStruct, BStructMember};
use bstruct::{CompileError, build_directory};
use std::collections::BTreeMap;

type TypeName = String;
type MemberName = String;
type EnumName = String;

#[derive(Debug)]
pub struct GameStructs {
  pub structs: BTreeMap<TypeName, GameStruct>,
  pub enums: BTreeMap<EnumName, GameEnum>,
}

impl GameStructs {
  pub fn new_empty() -> Self {
    Self {
      structs: BTreeMap::new(),
      enums: BTreeMap::new(),
    }
  }

  pub fn insert_struct(&mut self, struct_: &GameStruct) {
    self.structs.insert(struct_.name.clone(), struct_.clone());
  }

  pub fn insert_enum(&mut self, enum_: &GameEnum) {
    self.enums.insert(enum_.name.clone(), enum_.clone());
  }

  pub fn load_from_dir(&mut self, dir: &str) -> Result<(), String> {
    // walk the directory tree and find all .bs files
    // for each file, parse it and link it
    // add the structs and enums to the resource
    let compile = build_directory(dir);

    let compile_result = match compile {
      Ok(it) => it,
      Err(err) => match err {
        CompileError::ReadError(it) => return Err(format!("Read error: {}", it)),
        CompileError::ParseError(it) => return Err(format!("Parse error: {:?}", it)),
        CompileError::LinkError(it) => return Err(format!("Link error: {:?}", it)),
      },
    };

    for bstruct in compile_result.structs.iter() {
      self.insert_struct(&GameStruct::new(bstruct));
    }

    for benum in compile_result.enums.iter() {
      self.insert_enum(&GameEnum::new(benum));
    }

    Ok(())
  }

  pub fn get_struct_by_name(&self, name: &str) -> Option<GameStruct> {
    self.structs.get(name).cloned()
  }

  pub fn get_enum_by_name(&self, name: &str) -> Option<GameEnum> {
    self.enums.get(name).cloned()
  }
}

#[derive(Clone, Debug)]
pub struct GameEnum {
  pub name: TypeName,
  #[allow(unused)]
  pub size: i64,
  // name <-> value
  pub values: BiBTreeMap<EnumName, i64>,
}

impl GameEnum {
  pub fn new(benum: &BEnum) -> Self {
    let mut values = BiBTreeMap::new();
    for e in benum.values.iter() {
      values.insert(e.name.value.clone(), e.value.value());
    }

    Self {
      name: benum.name.value.clone(),
      size: benum.ext.size,
      values,
    }
  }

  #[allow(unused)]
  pub fn get_value_by_name(&self, name: &str) -> Option<i64> {
    self.values.get_by_left(name).cloned()
  }

  pub fn get_name_by_value(&self, value: i64) -> Option<String> {
    self.values.get_by_right(&value).cloned()
  }
}

#[derive(Clone, Debug)]
pub struct GameStruct {
  pub name: TypeName,
  pub size: i64,
  #[allow(unused)]
  pub vtable_address: Option<i64>,
  pub extends: Vec<TypeName>,
  pub members_by_offset: BTreeMap<i64, GameMember>,
  pub members_by_name: BTreeMap<MemberName, GameMember>,
}

impl GameStruct {
  pub fn new(bstruct: &BStruct) -> Self {
    let mut res = Self {
      name: bstruct.name.value.clone(),
      size: bstruct.size.unwrap().value(), // TODO: fix this to not be optional in bstruct... bstruct needs a new api
      vtable_address: bstruct.vtable.map(|it| it.value()),
      extends: bstruct.ext.iter().map(|it| it.value.clone()).collect(),
      members_by_offset: BTreeMap::new(),
      members_by_name: BTreeMap::new(),
    };

    for member in bstruct.members.iter() {
      res.insert_member(&GameMember::new(member))
    }

    res
  }

  pub fn insert_member(&mut self, member: &GameMember) {
    self.members_by_offset.insert(member.offset, member.clone());
    self
      .members_by_name
      .insert(member.name.clone(), member.clone());
  }

  pub fn get_member_by_name(&self, game_structs: &GameStructs, name: &str) -> Option<GameMember> {
    if let Some(member) = self.members_by_name.get(name) {
      return Some(member.clone());
    }
    for parent_name in self.extends.iter() {
      if let Some(parent) = game_structs.get_struct_by_name(parent_name) {
        if let Some(member) = parent.get_member_by_name(game_structs, name) {
          return Some(member);
        }
      }
    }
    None
  }

  pub fn extends(&self, game_structs: &GameStructs, type_name: &str) -> bool {
    for parent_name in self.extends.iter() {
      if parent_name == type_name {
        return true;
      }
      if let Some(parent) = game_structs.get_struct_by_name(parent_name) {
        if parent.extends(game_structs, type_name) {
          return true;
        }
      }
    }
    false
  }
}

#[derive(Clone, Debug)]
pub struct GameMember {
  pub type_name: String,
  pub name: MemberName,
  pub offset: i64,
  pub bit: Option<i64>,
  pub bit_length: Option<i64>,
  pub array_length: Option<i64>,
  pub pointer: bool,
}

impl GameMember {
  pub fn new(member: &BStructMember) -> Self {
    GameMember {
      type_name: member.type_name.value.clone(),
      name: member.name.value.clone(),
      offset: member.offset.value(),
      bit: member.bit.map(|it| it.value()),
      bit_length: member.bit_length.map(|it| it.value()),
      array_length: member.array_length.map(|it| it.value()),
      pointer: member.pointer,
    }
  }

  #[allow(unused)]
  pub fn get_type(&self, game_structs: &GameStructs) -> Option<GameStruct> {
    game_structs.get_struct_by_name(&self.type_name)
  }
}

/// u64/i64 currently ignored.
pub fn primitive_size(type_name: &str) -> u32 {
  match type_name {
    "u8" | "i8" | "bool" => 1,
    "u16" | "i16" => 2,
    "u32" | "i32" | "f32" => 4,
    "f64" => 8,
    _ => 4,
  }
}

#[derive(Clone, Debug)]
pub struct GameInstance {
  pub address: u32,
  pub type_name: String,
  /// Bitfield start bit / length carried from the `GameMember` this instance was
  /// resolved from. `None` for struct roots and any non-bitfield member; only the integer `read_u*` reads
  /// consult them.
  pub bit: Option<i64>,
  pub bit_length: Option<i64>,
  /// Array length carried from the `GameMember` this instance was resolved from
  /// `None` for struct roots, non-array members, and every instance produced by [`GameInstance::element`].
  pub array_length: Option<i64>,
  /// Whether the `GameMember` this instance was resolved from was a pointer
  /// member. `false` for struct roots ([`GameInstance::new`] / [`GameInstance::with_bitfield`])
  /// and every instance produced by [`GameInstance::element`].
  pub pointer: bool,
}

impl GameInstance {
  pub fn new(address: u32, type_name: String) -> Self {
    Self {
      address,
      type_name,
      bit: None,
      bit_length: None,
      array_length: None,
      pointer: false,
    }
  }

  #[allow(dead_code)]
  pub fn with_bitfield(
    address: u32,
    type_name: String,
    bit: Option<i64>,
    bit_length: Option<i64>,
  ) -> Self {
    Self {
      address,
      type_name,
      bit,
      bit_length,
      array_length: None,
      pointer: false,
    }
  }

  /// Build an instance from a resolved `GameMember` at `address`, carrying its
  /// bitfield and array-length metadata. The pointer auto-deref is handled by
  /// the caller (`get_member`).
  fn with_member(address: u32, member: &GameMember) -> Self {
    Self {
      address,
      type_name: member.type_name.clone(),
      bit: member.bit,
      bit_length: member.bit_length,
      array_length: member.array_length,
      pointer: member.pointer,
    }
  }

  /// a malformed `.bs` could hand us a negative value, so clamp before the cast.
  fn bit_u32(&self) -> u32 {
    self.bit.unwrap_or(0).max(0) as u32
  }

  fn bit_length_u32(&self) -> u32 {
    self.bit_length.unwrap_or(0).max(0) as u32
  }

  pub fn get_type(&self, ctx: &Ctx) -> Option<GameStruct> {
    ctx.structs.get_struct_by_name(&self.type_name)
  }

  /// True if this instance's own type *is* `class_name`, or its type transitively extends it.
  /// A type name with no matching `.bs` struct only matches on the identity check.
  /// Delegates to [`GameStruct::extends`] for the recursion.
  pub fn extends_class(&self, ctx: &Ctx, class_name: &str) -> bool {
    if self.type_name == class_name {
      return true;
    }
    match self.get_type(ctx) {
      Some(s) => s.extends(ctx.structs, class_name),
      None => false,
    }
  }

  /// Fallible member lookup: the `Option` form of [`GameInstance::member`], for
  /// call sites where a missing member is a legitimate outcome (optional field,
  /// speculative probe). Auto-derefs pointer members.
  pub fn get_member(&self, ctx: &Ctx, name: &str) -> Option<GameInstance> {
    let struct_ = self.get_type(ctx)?;
    let member = struct_.get_member_by_name(ctx.structs, name)?;
    let mut addr = self.address + member.offset as u32;
    if member.pointer {
      addr = ctx.mem.read_u32(addr)?
    }
    Some(GameInstance::with_member(addr, &member))
  }

  /// Panicking-on-absence was the *documented, intended* behavior: a
  /// missing member here means a typo in a `.bs` file or a call site, i.e. a bug
  /// Use [`GameInstance::get_member`] when a miss is legitimate.
  pub fn member(&self, ctx: &Ctx, name: &str) -> GameInstance {
    self
      .get_member(ctx, name)
      .unwrap_or_else(|| panic!("Unknown member {}.{}", self.type_name, name))
  }

  /// Stride of one element of this instance's type, in bytes. A struct's `size`,
  /// else `primitive_size`. A negative schema `size` clamps to 0.
  pub fn element_size(&self, ctx: &Ctx) -> u32 {
    ctx
      .structs
      .get_struct_by_name(&self.type_name)
      .map(|s| s.size.max(0) as u32)
      .unwrap_or_else(|| primitive_size(&self.type_name))
  }

  /// The `index`-th array element: a fresh instance at
  /// `self.address + index * element_size`, same `type_name`, with `bit` /
  /// `bit_length` / `array_length` all cleared.
  pub fn element(&self, ctx: &Ctx, index: u32) -> GameInstance {
    GameInstance::new(
      self
        .address
        .wrapping_add(index.wrapping_mul(self.element_size(ctx))),
      self.type_name.clone(),
    )
  }

  pub fn read_u8(&self, ctx: &Ctx) -> Option<u8> {
    ctx
      .mem
      .read_u8_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u16(&self, ctx: &Ctx) -> Option<u16> {
    ctx
      .mem
      .read_u16_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u32(&self, ctx: &Ctx) -> Option<u32> {
    ctx
      .mem
      .read_u32_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u64(&self, ctx: &Ctx) -> Option<u64> {
    ctx
      .mem
      .read_u64_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_bool(&self, ctx: &Ctx) -> Option<bool> {
    self.read_u8(ctx).map(|v| v != 0)
  }

  pub fn read_f32(&self, ctx: &Ctx) -> Option<f32> {
    ctx.mem.read_f32(self.address)
  }

  pub fn read_f64(&self, ctx: &Ctx) -> Option<f64> {
    ctx.mem.read_f64(self.address)
  }

  pub fn read_string(&self, ctx: &Ctx) -> Option<String> {
    ctx.mem.read_string(self.address)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;

  fn member(name: &str, type_name: &str, offset: i64) -> GameMember {
    GameMember {
      type_name: type_name.to_string(),
      name: name.to_string(),
      offset,
      bit: None,
      bit_length: None,
      array_length: None,
      pointer: false,
    }
  }

  fn game_struct(name: &str, extends: &[&str], members: &[GameMember]) -> GameStruct {
    game_struct_sized(name, 0, extends, members)
  }

  fn game_struct_sized(
    name: &str,
    size: i64,
    extends: &[&str],
    members: &[GameMember],
  ) -> GameStruct {
    let mut s = GameStruct {
      name: name.to_string(),
      size,
      vtable_address: None,
      extends: extends.iter().map(|it| it.to_string()).collect(),
      members_by_offset: BTreeMap::new(),
      members_by_name: BTreeMap::new(),
    };
    for m in members {
      s.insert_member(m);
    }
    s
  }

  /// `C : B : A`, plus an unrelated `D : A` and a bare `X`.
  fn chain() -> GameStructs {
    let mut structs = GameStructs::new_empty();
    structs.insert_struct(&game_struct("A", &[], &[member("a_field", "uint", 0x0)]));
    structs.insert_struct(&game_struct("B", &["A"], &[member("b_field", "uint", 0x4)]));
    structs.insert_struct(&game_struct("C", &["B"], &[member("c_field", "uint", 0x8)]));
    structs.insert_struct(&game_struct("D", &["A"], &[]));
    structs.insert_struct(&game_struct("X", &[], &[]));
    structs
  }

  #[test]
  fn extends_is_transitive() {
    let structs = chain();
    let c = structs.get_struct_by_name("C").unwrap();
    // direct parent
    assert!(c.extends(&structs, "B"));
    // grandparent — the bug: only found when recursion passes the original target
    assert!(c.extends(&structs, "A"));
  }

  #[test]
  fn extends_negative_cases() {
    let structs = chain();
    let c = structs.get_struct_by_name("C").unwrap();
    assert!(!c.extends(&structs, "X"));
    assert!(!c.extends(&structs, "D"));
    assert!(!c.extends(&structs, "C"));
    // sibling branch: D extends A but not B or C
    let d = structs.get_struct_by_name("D").unwrap();
    assert!(d.extends(&structs, "A"));
    assert!(!d.extends(&structs, "B"));
  }

  #[test]
  fn get_member_by_name_resolves_through_chain() {
    let structs = chain();
    let c = structs.get_struct_by_name("C").unwrap();
    // declared on C
    assert_eq!(
      c.get_member_by_name(&structs, "c_field").unwrap().offset,
      0x8
    );
    // declared on B
    assert_eq!(
      c.get_member_by_name(&structs, "b_field").unwrap().offset,
      0x4
    );
    // declared on grandparent A
    assert_eq!(
      c.get_member_by_name(&structs, "a_field").unwrap().offset,
      0x0
    );
    // absent everywhere
    assert!(c.get_member_by_name(&structs, "missing").is_none());
  }

  /// Skip-if-absent loader for the offline BE dump — same contract as the
  /// `game_memory.rs` tests.
  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping prime_structs mem1.raw tests: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  #[test]
  fn game_instance_reads_match_raw_memory() {
    let Some(mem) = load_mem1() else { return };
    let structs = GameStructs::new_empty();
    let ctx = Ctx::new(&structs, &mem);

    // A plain (non-bitfield) instance at the disc header.
    let inst = GameInstance::new(0x8000_0000, "uint".to_string());
    assert_eq!(inst.read_u32(&ctx), mem.read_u32(0x8000_0000));
    assert_eq!(inst.read_u16(&ctx), mem.read_u16(0x8000_0000));
    assert_eq!(inst.read_u8(&ctx), mem.read_u8(0x8000_0000));
    assert_eq!(inst.read_u64(&ctx), mem.read_u64(0x8000_0000));
    assert_eq!(inst.read_bool(&ctx), Some(true));
    assert_eq!(inst.read_string(&ctx), Some("GM8E01".to_string()));

    let fp = GameInstance::new(0x8000_001C, "float".to_string());
    assert_eq!(fp.read_f32(&ctx), mem.read_f32(0x8000_001C));
    assert_eq!(fp.read_f64(&ctx), mem.read_f64(0x8000_001C));
  }

  #[test]
  fn game_instance_bitfield_masking() {
    let Some(mem) = load_mem1() else { return };
    let structs = GameStructs::new_empty();
    let ctx = Ctx::new(&structs, &mem);

    // u32 @ 0x8000_001C == 0xC233_9F3D; (v >> 4) & 0xF == 0x3, (v >> 0) & 0xFF == 0x3D.
    let bf = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(4), Some(4));
    assert_eq!(bf.read_u32(&ctx), mem.read_u32_bits(0x8000_001C, 4, 4));
    assert_eq!(bf.read_u32(&ctx), Some(0x3));

    let bf2 = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(0), Some(8));
    assert_eq!(bf2.read_u32(&ctx), Some(0x3D));

    // Negative bit / length from a malformed schema clamp to 0 (whole value).
    let bad = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(-3), Some(-1));
    assert_eq!(bad.read_u32(&ctx), mem.read_u32(0x8000_001C));

    // f32 ignores bit fields entirely.
    let bf_f = GameInstance::with_bitfield(0x8000_001C, "float".to_string(), Some(4), Some(4));
    assert_eq!(bf_f.read_f32(&ctx), mem.read_f32(0x8000_001C));
  }

  #[test]
  fn game_instance_oob_reads_are_none() {
    let Some(mem) = load_mem1() else { return };
    let structs = GameStructs::new_empty();
    let ctx = Ctx::new(&structs, &mem);
    let oob = GameInstance::new(0x8190_0000, "uint".to_string());
    assert_eq!(oob.read_u8(&ctx), None);
    assert_eq!(oob.read_u32(&ctx), None);
    assert_eq!(oob.read_u64(&ctx), None);
    assert_eq!(oob.read_bool(&ctx), None);
    assert_eq!(oob.read_f32(&ctx), None);
    assert_eq!(oob.read_string(&ctx), None);
  }

  #[test]
  fn get_member_carries_bitfield() {
    let mut structs = GameStructs::new_empty();
    let mut m = member("flags", "uint", 0x0);
    m.bit = Some(2);
    m.bit_length = Some(3);
    structs.insert_struct(&game_struct("S", &[], &[m]));

    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "S".to_string());
    let field = root.get_member(&ctx, "flags").unwrap();
    assert_eq!(field.bit, Some(2));
    assert_eq!(field.bit_length, Some(3));
  }

  #[test]
  fn get_member_carries_pointer() {
    let mut structs = GameStructs::new_empty();
    let mut ptr = member("target", "Target", 0x4);
    ptr.pointer = true;
    structs.insert_struct(&game_struct(
      "Owner",
      &[],
      &[ptr, member("plain", "u32", 0x0)],
    ));
    structs.insert_struct(&game_struct("Target", &[], &[]));

    let mut mem = GameMemory::new();
    // Pointer slot at 0x8000_0004 -> 0x8000_1000.
    mem.data[0x4..0x8].copy_from_slice(&0x8000_1000u32.to_be_bytes());
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "Owner".to_string());

    let via_ptr = root.get_member(&ctx, "target").unwrap();
    assert!(via_ptr.pointer, "pointer bit must survive the deref");
    assert_eq!(via_ptr.address, 0x8000_1000);

    let plain = root.get_member(&ctx, "plain").unwrap();
    assert!(!plain.pointer);

    // Ctors / element() never set it.
    assert!(!GameInstance::new(0x8000_0000, "u32".to_string()).pointer);
    assert!(!GameInstance::with_bitfield(0x8000_0000, "u32".to_string(), Some(0), Some(4)).pointer);
    assert!(!via_ptr.element(&ctx, 1).pointer);
  }

  #[test]
  fn primitive_size_table() {
    assert_eq!(primitive_size("u8"), 1);
    assert_eq!(primitive_size("i8"), 1);
    assert_eq!(primitive_size("bool"), 1);
    assert_eq!(primitive_size("u16"), 2);
    assert_eq!(primitive_size("i16"), 2);
    assert_eq!(primitive_size("u32"), 4);
    assert_eq!(primitive_size("i32"), 4);
    assert_eq!(primitive_size("f32"), 4);
    assert_eq!(primitive_size("f64"), 8);
    assert_eq!(primitive_size("u64"), 4);
    assert_eq!(primitive_size("i64"), 4);
    // unknown type name -> default 4.
    assert_eq!(primitive_size("CVector3f"), 4);
  }

  #[test]
  fn element_indexing_stride_and_field_clearing() {
    let mut structs = GameStructs::new_empty();
    structs.insert_struct(&game_struct_sized("Foo", 12, &[], &[]));
    let mut prims = member("prims", "u32", 0x10);
    prims.array_length = Some(8);
    let mut foos = member("foos", "Foo", 0x40);
    foos.array_length = Some(4);
    structs.insert_struct(&game_struct("Container", &[], &[prims, foos]));

    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "Container".to_string());

    let prim_arr = root.get_member(&ctx, "prims").unwrap();
    assert_eq!(prim_arr.array_length, Some(8));
    assert_eq!(prim_arr.element_size(&ctx), 4);
    let p3 = prim_arr.element(&ctx, 3);
    assert_eq!(p3.address, 0x8000_0000 + 0x10 + 3 * 4);
    assert_eq!(p3.array_length, None);
    assert_eq!(p3.bit, None);
    assert_eq!(p3.bit_length, None);
    assert_eq!(p3.type_name, "u32");

    let foo_arr = root.get_member(&ctx, "foos").unwrap();
    assert_eq!(foo_arr.array_length, Some(4));
    assert_eq!(foo_arr.element_size(&ctx), 12);
    let f3 = foo_arr.element(&ctx, 3);
    assert_eq!(f3.address, 0x8000_0000 + 0x40 + 3 * 12);
    assert_eq!(f3.array_length, None);
    assert_eq!(f3.type_name, "Foo");
  }

  #[test]
  fn element_reads_match_raw_memory() {
    let Some(mem) = load_mem1() else { return };

    let mut structs = GameStructs::new_empty();
    let mut words = member("words", "u32", 0x0);
    words.array_length = Some(6);
    structs.insert_struct(&game_struct("Header", &[], &[words]));

    let ctx = Ctx::new(&structs, &mem);
    let base = 0x8000_0000;
    let root = GameInstance::new(base, "Header".to_string());
    let arr = root.get_member(&ctx, "words").unwrap();
    for n in 0..6u32 {
      let el = arr.element(&ctx, n);
      assert_eq!(el.address, base + n * 4);
      assert_eq!(el.read_u32(&ctx), mem.read_u32(base + n * 4));
    }
  }

  #[test]
  fn get_member_by_name_prefers_local_override() {
    let mut structs = GameStructs::new_empty();
    structs.insert_struct(&game_struct("Base", &[], &[member("val", "uint", 0x10)]));
    structs.insert_struct(&game_struct(
      "Derived",
      &["Base"],
      &[member("val", "float", 0x20)],
    ));
    let derived = structs.get_struct_by_name("Derived").unwrap();
    let m = derived.get_member_by_name(&structs, "val").unwrap();
    assert_eq!(m.offset, 0x20);
    assert_eq!(m.type_name, "float");
  }

  #[test]
  fn member_matches_get_member_when_present() {
    let structs = chain();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "C".to_string());

    let via_get = root.get_member(&ctx, "b_field").unwrap();
    let via_member = root.member(&ctx, "b_field");
    assert_eq!(via_member.address, via_get.address);
    assert_eq!(via_member.type_name, via_get.type_name);
  }

  #[test]
  #[should_panic(expected = "Unknown member")]
  fn member_panics_on_typo() {
    let structs = chain();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "C".to_string());
    root.member(&ctx, "b_feild");
  }

  #[test]
  fn extends_class_resolves_transitive_inheritance() {
    let structs = chain();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);

    let c = GameInstance::new(0x8000_0000, "C".to_string());
    // identity
    assert!(c.extends_class(&ctx, "C"));
    // direct parent
    assert!(c.extends_class(&ctx, "B"));
    // grandparent (transitive)
    assert!(c.extends_class(&ctx, "A"));
    // unrelated
    assert!(!c.extends_class(&ctx, "X"));
    assert!(!c.extends_class(&ctx, "D"));

    // A type name with no `.bs` struct: only the identity check can match.
    let unknown = GameInstance::new(0x8000_0000, "CGameCamera".to_string());
    assert!(unknown.extends_class(&ctx, "CGameCamera"));
    assert!(!unknown.extends_class(&ctx, "A"));
  }

  /// `.member(..).member(..)` composes — proves the panicking form chains.
  #[test]
  fn member_chains_two_levels() {
    let mut structs = GameStructs::new_empty();
    structs.insert_struct(&game_struct("Leaf", &[], &[member("c", "u32", 0x4)]));
    structs.insert_struct(&game_struct("Mid", &[], &[member("b", "Leaf", 0x8)]));
    structs.insert_struct(&game_struct("Top", &[], &[member("a", "Mid", 0x10)]));

    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let root = GameInstance::new(0x8000_0000, "Top".to_string());
    let leaf = root.member(&ctx, "a").member(&ctx, "b").member(&ctx, "c");
    assert_eq!(leaf.address, 0x8000_0000 + 0x10 + 0x8 + 0x4);
    assert_eq!(leaf.type_name, "u32");
  }
}
