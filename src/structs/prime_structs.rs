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
        if parent.extends(game_structs, parent_name) {
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
}

impl GameInstance {
  pub fn new(address: u32, type_name: String) -> Self {
    Self { address, type_name }
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
    Some(GameInstance::new(addr, member.type_name.clone()))
  }

  // this makes it cleaner to use
  pub fn read_u32(&self, mem: &GameMemory) -> Option<u32> {
    mem.read_u32(self.address)
  }
}
