mod app;
mod ctx;
mod inspector;
mod mem;
mod scene;
mod structs;
mod world;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  app::run()
}
