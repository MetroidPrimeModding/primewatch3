use bevy::prelude::Resource;
use bstruct::bstruct_link::{BEnum, BStruct};
use bstruct::{CompileError, build_directory};

#[derive(Resource)]
pub struct PrimeStructs {
  pub structs: Vec<BStruct>,
  pub enums: Vec<BEnum>,
}

impl PrimeStructs {
  pub fn new_empty() -> Self {
    Self {
      structs: Vec::new(),
      enums: Vec::new(),
    }
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

    self.structs = compile_result.structs;
    self.enums = compile_result.enums;

    Ok(())
  }
}
