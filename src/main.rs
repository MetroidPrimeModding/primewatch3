mod mem;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, egui};
use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;

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


  App::new()
    .add_plugins(default_plugins)
    .add_plugins(EguiPlugin)
    .add_systems(Update, ui_example_system)
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
