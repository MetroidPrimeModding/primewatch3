use std::fs;
use std::io::Read;

const DOLPHIN_MEMORY_SIZE: u32 = 0x1800000u32;

pub struct GameMemory {
  pub data: [u8; DOLPHIN_MEMORY_SIZE as usize],
}

impl GameMemory {
  pub fn new() -> Self {
    GameMemory {
      data: [0; DOLPHIN_MEMORY_SIZE as usize],
    }
  }

  pub fn load_from_file(&mut self, path: &str) -> Result<(), std::io::Error> {
    let file = fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);

    // could use read_exact(&mut self.data), but this is safer, since it won't override the existing data
    let mut contents = vec![0; DOLPHIN_MEMORY_SIZE as usize];
    reader.read_exact(&mut contents)?;
    self.data = contents.try_into().unwrap();

    Ok(())
  }
}

impl GameMemory {
  // the gamecube is big endian, and addresses start at 0x8000_0000
  // Both memory dumps and Dolphin store it in a different spot
  fn address_to_offset(address: u32) -> usize {
    (address & 0x7FFFFFFFu32) as usize
  }

  pub fn read_u8(&self, address: u32) -> Option<u8> {
    let offset = GameMemory::address_to_offset(address);
    self.data.get(offset).copied()
  }

  pub fn read_u16(&self, address: u32) -> Option<u16> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..].try_into().ok().map(u16::from_be_bytes)
  }

  pub fn read_u32(&self, address: u32) -> Option<u32> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..].try_into().ok().map(u32::from_be_bytes)
  }

  pub fn read_u64(&self, address: u32) -> Option<u64> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..].try_into().ok().map(u64::from_be_bytes)
  }

  pub fn read_f32(&self, address: u32) -> Option<f32> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..].try_into().ok().map(f32::from_be_bytes)
  }

  pub fn read_f64(&self, address: u32) -> Option<f64> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..].try_into().ok().map(f64::from_be_bytes)
  }
}

trait MemoryAccess {}
