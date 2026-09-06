use crate::structs::prime_structs::GameInstance;
use crate::structs::prime_structs::GameMember;
use rhai::plugin::*;
use std::ops::AddAssign;
use std::rc::Rc;

pub fn register_scripting_modules(engine: &mut rhai::Engine) {
  engine.build_type::<GameMember>();
  engine.build_type::<CustomInspectorWindow>();
  engine.register_global_module(exported_module!(primewatch3_module).into());
}

#[derive(Clone)]
pub enum CustomInspectorRow {
  GameInstance(Rc<GameInstance>),
  Text(String),
}


#[derive(Default, Clone, CustomType)]
pub struct CustomInspectorWindow {
  pub title: String,
  pub rows: Vec<CustomInspectorRow>,
  // todo: window position config?
}


impl CustomInspectorWindow {
  pub fn new(title: String) -> Self {
    CustomInspectorWindow {
      title,
      rows: vec![],
    }
  }

  pub fn add_instance(&mut self, member: Rc<GameInstance>) {
    self.rows.push(CustomInspectorRow::GameInstance(member));
  }

  pub fn add_string(&mut self, text: String) {
    self.rows.push(CustomInspectorRow::Text(text));
  }
}

impl AddAssign<Rc<GameInstance>> for CustomInspectorWindow {
  fn add_assign(&mut self, rhs: Rc<GameInstance>) {
    self.rows.push(CustomInspectorRow::GameInstance(rhs));
  }
}

impl AddAssign<String> for CustomInspectorWindow {
  fn add_assign(&mut self, rhs: String) {
    self.rows.push(CustomInspectorRow::Text(rhs));
  }
}

#[export_module]
pub mod primewatch3_module {
  pub fn new_inspector_window(title: String) -> CustomInspectorWindow {
    CustomInspectorWindow::new(title)
  }

  pub fn get_player_entity() -> GameInstance {
    todo!()
  }

  pub fn get_entity_by_editor_id() -> GameInstance {
    todo!()
  }

  pub fn get_entity_by_unique_id() -> GameInstance {
    todo!()
  }
}
