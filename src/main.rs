mod app;
mod ctx;
mod mem;
mod scene;
mod structs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  app::run()
}
