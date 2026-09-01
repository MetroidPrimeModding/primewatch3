mod mem;
mod structs;

use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::structs::prime_structs::GameStructs;

fn main() {
  let mem = GameMemory::new();
  let v = mem.read_u16(0x80000000);
  println!("Value: {:?}", v);

  let mut dma = DolphinMemoryAccess::new();
  let pids = dma.get_dolphin_pids();
  println!("PIDs: {:?}", pids);

  let mut structs = GameStructs::new_empty();
  let load_result = structs.load_from_dir("prime_defs");
  match load_result {
    Ok(_) => println!(
      "Loaded {} structs and {} enums",
      structs.structs.len(),
      structs.enums.len()
    ),
    Err(err) => println!("Error loading structs: {}", err),
  }
}
