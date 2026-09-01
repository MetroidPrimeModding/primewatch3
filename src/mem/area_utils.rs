use crate::mem::game_memory::GameMemory;
use crate::mem::globals::get_state_manager;
use crate::structs::prime_structs::{GameInstance, GameStructs};

// fn get_areas(game_structs: &GameStructs, mem: &GameMemory) -> Vec<GameInstance> {
//   let state_manager = get_state_manager();
//   let world = state_manager.get_member(game_structs, mem, "world");
//   if world.is_none() {
//     return vec![];
//   }
//   let world = world.unwrap();
//   let areas = world.get_member(game_structs, mem, "areas");
//   if areas.is_none() {
//     return vec![];
//   }
//   let areas = areas.unwrap();
//
//   // loop thru the vector
//   let end = areas.get_member(game_structs, mem, "end").unwrap().read_u32(mem).unwrap();
//   //    let size = areas.member_by_name("size").read_u32();
//
//   let mut result = Vec::new();
//
//   let first = areas["first"];
//   let size_per = GameDefinitions::struct_by_name(&first.r#type).size;
//   for i in 0..end {
//     let mut vec_item = first.clone();
//     vec_item.offset += size_per * i;
//     let mut area = vec_item["value"].clone();
//     area.name = format!("area {}", i);
//     result.push(area);
//   }
//   result
// }
