mod app;
mod ctx;
mod defs;
mod gl;
mod inspector;
mod mem;
mod object_filter;
mod structs;
mod ui_state;
mod world;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  app::run()
}
