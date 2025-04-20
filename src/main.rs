mod mem;
mod structs;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, egui};
use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::structs::prime_structs::GameStructs;

fn main() {
  let default_plugins = DefaultPlugins.set(WindowPlugin {
    primary_window: Some(Window {
      title: "Prime Watch 3".to_string(),
      ..Default::default()
    }),
    ..Default::default()
  });

  let mem = GameMemory::new();
  let v = mem.read_u16(0x80000000);
  println!("Value: {:?}", v);

  let mut dma = DolphinMemoryAccess::new();
  let pids = dma.get_dolphin_pids();
  println!("PIDs: {:?}", pids);

  let mut structs = GameStructs::new_empty();
  let loadResult = structs.load_from_dir("prime_defs");
  match loadResult {
    Ok(_) => println!("Loaded {} structs and {} enums", structs.structs.len(), structs.enums.len()),
    Err(err) => println!("Error loading structs: {}", err),
  }

  App::new()
    .add_plugins(default_plugins)
    .add_plugins(EguiPlugin)
    .insert_resource(structs)
    .insert_resource(mem)
    .add_systems(Update, ui_example_system)
    .add_systems(Startup, do_memory_parse)
    .run();
}

fn ui_example_system(mut contexts: EguiContexts) {
  egui::Window::new("Hello").show(contexts.ctx_mut(), |ui| {
    ui.label("world");

    if ui.button("Click me!").clicked() {
      println!("Button clicked!");
    }
  });
}


fn do_memory_parse(mem: Res<GameMemory>) {
  // parse out collision
  
}