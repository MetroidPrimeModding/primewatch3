use crate::structs::prime_structs::GameInstance;
use std::string::ToString;

pub fn get_state_manager() -> GameInstance {
  GameInstance::new(0x8045A1A8, "CStateManager".to_string())
}

pub fn get_main() -> GameInstance {
  GameInstance::new(0x80457560, "CMain".to_string())
}
