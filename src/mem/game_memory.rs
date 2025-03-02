const DOLPHIN_MEMORY_SIZE: u32 = 0x1800000u32;
pub struct GameMemory {
  data: [u8; DOLPHIN_MEMORY_SIZE as usize],
}

impl GameMemory {
  pub fn new() -> Self {
    GameMemory {
      data: [0; DOLPHIN_MEMORY_SIZE as usize],
    }
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
    self.data[offset..]
      .try_into().ok().map(u16::from_be_bytes)
  }

  pub fn read_u32(&self, address: u32) -> Option<u32> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..]
      .try_into().ok().map(u32::from_be_bytes)
  }

  pub fn read_u64(&self, address: u32) -> Option<u64> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..]
      .try_into().ok().map(u64::from_be_bytes)
  }

  pub fn read_f32(&self, address: u32) -> Option<f32> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..]
      .try_into().ok().map(f32::from_be_bytes)
  }

  pub fn read_f64(&self, address: u32) -> Option<f64> {
    let offset = GameMemory::address_to_offset(address);
    self.data[offset..]
      .try_into().ok().map(f64::from_be_bytes)
  }
}

trait MemoryAccess {}
