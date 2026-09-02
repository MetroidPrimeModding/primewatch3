mod app;
mod ctx;
mod gl;
mod inspector;
mod mem;
mod structs;
mod world;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  app::run()
}
