mod app;
mod mem;
mod structs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  app::run()
}
