mod app;
mod ctx;
mod defs;
mod gl;
mod inspector;
mod mem;
mod object_filter;
mod structs;
mod toast;
mod trace;
mod ui_state;
mod world;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  if let Some(result) = trace::run_if_requested() {
    return result.map_err(Into::into);
  }
  app::run()
}
