use crate::mem::game_memory::GameMemory;
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

  pub fn get_type(&self, game_structs: &GameStructs) -> Option<GameStruct> {
    game_structs.get_struct_by_name(&self.type_name)
  }
}

#[derive(Clone, Debug)]
pub struct GameInstance {
  pub address: u32,
  pub type_name: String,
  /// Bitfield start bit / length carried from the `GameMember` this instance was
  /// resolved from (C++ `GameMember::bit` / `GameMember::bitLength`). `None` for
  /// struct roots and any non-bitfield member; only the integer `read_u*` reads
  /// consult them. See C++ `GameDefinitions::getBits`.
  pub bit: Option<i64>,
  pub bit_length: Option<i64>,
}

impl GameInstance {
  pub fn new(address: u32, type_name: String) -> Self {
    Self {
      address,
      type_name,
      bit: None,
      bit_length: None,
    }
  }

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
    }
  }

  /// C++ `getBits` takes `optional<uint32_t>` and calls `.value_or(0)`; a
  /// malformed `.bs` could hand us a negative value, so clamp before the cast.
  fn bit_u32(&self) -> u32 {
    self.bit.unwrap_or(0).max(0) as u32
  }

  fn bit_length_u32(&self) -> u32 {
    self.bit_length.unwrap_or(0).max(0) as u32
  }

  pub fn get_type(&self, structs: &GameStructs) -> Option<GameStruct> {
    structs.get_struct_by_name(&self.type_name)
  }

  pub fn get_member(
    &self,
    game_structs: &GameStructs,
    mem: &GameMemory,
    name: &str,
  ) -> Option<GameInstance> {
    let struct_ = self.get_type(game_structs)?;
    let member = struct_.get_member_by_name(game_structs, name)?;
    let mut addr = self.address + member.offset as u32;
    if member.pointer {
      addr = mem.read_u32(addr)?
    }
    Some(GameInstance::with_bitfield(
      addr,
      member.type_name.clone(),
      member.bit,
      member.bit_length,
    ))
  }

  /// Ports C++ `GameMember::read_*` (`GameDefinitions.cpp:246-283`). Integer
  /// reads route through `GameMemory::read_u*_bits` (C++ `getBits`); `f32`/`f64`/
  /// `string` take no bit masking, matching the C++.
  ///
  /// Deviation from C++: C++ reads are total (`getRealPtr` clamps OOB to 0).
  /// These return `Option` and do **not** substitute a default — defaulting is
  /// deferred to the P7 render callsites so the inspector can tell "unreadable"
  /// from "zero" and the reads compose with `?`.
  pub fn read_u8(&self, mem: &GameMemory) -> Option<u8> {
    mem.read_u8_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u16(&self, mem: &GameMemory) -> Option<u16> {
    mem.read_u16_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u32(&self, mem: &GameMemory) -> Option<u32> {
    mem.read_u32_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_u64(&self, mem: &GameMemory) -> Option<u64> {
    mem.read_u64_bits(self.address, self.bit_u32(), self.bit_length_u32())
  }

  pub fn read_bool(&self, mem: &GameMemory) -> Option<bool> {
    self.read_u8(mem).map(|v| v != 0)
  }

  pub fn read_f32(&self, mem: &GameMemory) -> Option<f32> {
    mem.read_f32(self.address)
  }

  pub fn read_f64(&self, mem: &GameMemory) -> Option<f64> {
    mem.read_f64(self.address)
  }

  pub fn read_string(&self, mem: &GameMemory) -> Option<String> {
    mem.read_string(self.address)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
    let mut s = GameStruct {
      name: name.to_string(),
      size: 0,
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
      .unwrap_or_else(|_| format!("{}/../primewatch2/mem1.raw", env!("CARGO_MANIFEST_DIR")));
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

    // A plain (non-bitfield) instance at the disc header.
    let inst = GameInstance::new(0x8000_0000, "uint".to_string());
    assert_eq!(inst.read_u32(&mem), mem.read_u32(0x8000_0000));
    assert_eq!(inst.read_u16(&mem), mem.read_u16(0x8000_0000));
    assert_eq!(inst.read_u8(&mem), mem.read_u8(0x8000_0000));
    assert_eq!(inst.read_u64(&mem), mem.read_u64(0x8000_0000));
    assert_eq!(inst.read_bool(&mem), Some(true));
    assert_eq!(inst.read_string(&mem), Some("GM8E01".to_string()));

    let fp = GameInstance::new(0x8000_001C, "float".to_string());
    assert_eq!(fp.read_f32(&mem), mem.read_f32(0x8000_001C));
    assert_eq!(fp.read_f64(&mem), mem.read_f64(0x8000_001C));
  }

  #[test]
  fn game_instance_bitfield_masking() {
    let Some(mem) = load_mem1() else { return };

    // u32 @ 0x8000_001C == 0xC233_9F3D; (v >> 4) & 0xF == 0x3, (v >> 0) & 0xFF == 0x3D.
    let bf = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(4), Some(4));
    assert_eq!(bf.read_u32(&mem), mem.read_u32_bits(0x8000_001C, 4, 4));
    assert_eq!(bf.read_u32(&mem), Some(0x3));

    let bf2 = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(0), Some(8));
    assert_eq!(bf2.read_u32(&mem), Some(0x3D));

    // Negative bit / length from a malformed schema clamp to 0 (whole value).
    let bad = GameInstance::with_bitfield(0x8000_001C, "uint".to_string(), Some(-3), Some(-1));
    assert_eq!(bad.read_u32(&mem), mem.read_u32(0x8000_001C));

    // f32 ignores bit fields entirely (matches C++).
    let bf_f = GameInstance::with_bitfield(0x8000_001C, "float".to_string(), Some(4), Some(4));
    assert_eq!(bf_f.read_f32(&mem), mem.read_f32(0x8000_001C));
  }

  #[test]
  fn game_instance_oob_reads_are_none() {
    let Some(mem) = load_mem1() else { return };
    let oob = GameInstance::new(0x8190_0000, "uint".to_string());
    assert_eq!(oob.read_u8(&mem), None);
    assert_eq!(oob.read_u32(&mem), None);
    assert_eq!(oob.read_u64(&mem), None);
    assert_eq!(oob.read_bool(&mem), None);
    assert_eq!(oob.read_f32(&mem), None);
    assert_eq!(oob.read_string(&mem), None);
  }

  #[test]
  fn get_member_carries_bitfield() {
    let mut structs = GameStructs::new_empty();
    let mut m = member("flags", "uint", 0x0);
    m.bit = Some(2);
    m.bit_length = Some(3);
    structs.insert_struct(&game_struct("S", &[], &[m]));

    let mem = GameMemory::new();
    let root = GameInstance::new(0x8000_0000, "S".to_string());
    let field = root.get_member(&structs, &mem, "flags").unwrap();
    assert_eq!(field.bit, Some(2));
    assert_eq!(field.bit_length, Some(3));
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
}
