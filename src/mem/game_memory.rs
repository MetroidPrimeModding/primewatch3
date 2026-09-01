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

  /// Bounded big-endian fixed-width fetch. Returns `None` when the `N` bytes at
  /// `address` fall outside the snapshot.
  ///
  /// C++ `GameMemory::getRealPtr` clamps an out-of-range masked address to
  /// offset `0` and reads garbage from the start of RAM; the Rust port instead
  /// reports the miss as `None`. Choosing a default (0 / empty) for a missing
  /// read is the caller's job (P4.2 `GameInstance`).
  fn read_bytes<const N: usize>(&self, address: u32) -> Option<[u8; N]> {
    let offset = GameMemory::address_to_offset(address);
    self.data.get(offset..offset + N)?.try_into().ok()
  }

  pub fn read_u8(&self, address: u32) -> Option<u8> {
    let offset = GameMemory::address_to_offset(address);
    self.data.get(offset).copied()
  }

  pub fn read_u16(&self, address: u32) -> Option<u16> {
    Some(u16::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_u32(&self, address: u32) -> Option<u32> {
    Some(u32::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_u64(&self, address: u32) -> Option<u64> {
    Some(u64::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_i8(&self, address: u32) -> Option<i8> {
    self.read_u8(address).map(|v| v as i8)
  }

  pub fn read_i16(&self, address: u32) -> Option<i16> {
    Some(i16::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_i32(&self, address: u32) -> Option<i32> {
    Some(i32::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_i64(&self, address: u32) -> Option<i64> {
    Some(i64::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_f32(&self, address: u32) -> Option<f32> {
    Some(f32::from_be_bytes(self.read_bytes(address)?))
  }

  pub fn read_f64(&self, address: u32) -> Option<f64> {
    Some(f64::from_be_bytes(self.read_bytes(address)?))
  }

  /// C++ `GameMember::read_bool` (`GameDefinitions.cpp:246`): a non-zero byte.
  pub fn read_bool(&self, address: u32) -> Option<bool> {
    Some(self.read_u8(address)? != 0)
  }

  /// C++ `GameMember::read_string` (`GameDefinitions.cpp:274`): NUL-terminated,
  /// max 255 bytes, raw ASCII (no byte-swap). Returns `None` only if the very
  /// first byte is out of range; a later OOB byte terminates the string like a
  /// NUL (mirrors C++ `getRealPtr` returning `0`).
  pub fn read_string(&self, address: u32) -> Option<String> {
    self.read_u8(address)?;
    let mut val = String::new();
    for i in 0..255u32 {
      match self.read_u8(address.wrapping_add(i)) {
        Some(0) | None => break,
        Some(byte) => val.push(byte as char),
      }
    }
    Some(val)
  }

  /// C++ `GameDefinitions::getBits` (`GameDefinitions.cpp:234-243`) with the
  /// shift/mask UB guarded for Rust: `bit_length == 0` (and, defensively,
  /// `bit_length >= width_bits`) means "the whole value"; a `bit` past the
  /// value width yields `0`.
  fn extract_bits(v: u64, bit: u32, bit_length: u32, width_bits: u32) -> u64 {
    let value_mask = if width_bits >= 64 {
      u64::MAX
    } else {
      (1u64 << width_bits) - 1
    };
    let field_mask = if bit_length == 0 || bit_length >= width_bits {
      u64::MAX
    } else {
      (1u64 << bit_length) - 1
    };
    let shifted = if bit >= width_bits { 0 } else { v >> bit };
    (shifted & field_mask) & value_mask
  }

  pub fn read_u8_bits(&self, address: u32, bit: u32, bit_length: u32) -> Option<u8> {
    let raw = self.read_u8(address)?;
    Some(GameMemory::extract_bits(raw as u64, bit, bit_length, 8) as u8)
  }

  pub fn read_u16_bits(&self, address: u32, bit: u32, bit_length: u32) -> Option<u16> {
    let raw = self.read_u16(address)?;
    Some(GameMemory::extract_bits(raw as u64, bit, bit_length, 16) as u16)
  }

  pub fn read_u32_bits(&self, address: u32, bit: u32, bit_length: u32) -> Option<u32> {
    let raw = self.read_u32(address)?;
    Some(GameMemory::extract_bits(raw as u64, bit, bit_length, 32) as u32)
  }

  pub fn read_u64_bits(&self, address: u32, bit: u32, bit_length: u32) -> Option<u64> {
    let raw = self.read_u64(address)?;
    Some(GameMemory::extract_bits(raw, bit, bit_length, 64))
  }
}

trait MemoryAccess {}

#[cfg(test)]
mod tests {
  use super::*;

  /// `GameMemory` is ~24 MiB; build it directly on the heap so tests don't blow
  /// the (2 MiB) test-thread stack that `GameMemory::new()`'s inline array would.
  fn blank() -> Box<GameMemory> {
    // SAFETY: `GameMemory` is a single `[u8; N]` field, for which all-zero is a
    // valid bit pattern.
    unsafe { Box::<GameMemory>::new_zeroed().assume_init() }
  }

  /// Skip-if-absent loader for the offline BE dump. Honours `PRIMEWATCH_MEM1_RAW`,
  /// else looks next to the crate at `../primewatch2/mem1.raw`.
  fn load_mem1() -> Option<Box<GameMemory>> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/../primewatch2/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping game_memory mem1.raw tests: {path} not found");
      return None;
    }
    // Read straight into the heap-allocated snapshot. (`GameMemory::load_from_file`
    // materialises a ~24 MiB array on the stack via `Vec::try_into` and would
    // overflow the test-thread stack — see the TODO in TASKS.md.)
    let bytes = std::fs::read(&path).expect("read mem1.raw");
    let mut mem = blank();
    let n = bytes.len().min(mem.data.len());
    mem.data[..n].copy_from_slice(&bytes[..n]);
    Some(mem)
  }

  #[test]
  fn reads_against_mem1() {
    let Some(mem) = load_mem1() else { return };

    // GameCube disc header
    assert_eq!(mem.read_u8(0x8000_0000), Some(0x47));
    assert_eq!(mem.read_u32(0x8000_0000), Some(0x474D_3845));
    assert_eq!(mem.read_string(0x8000_0000), Some("GM8E01".to_string()));
    assert_eq!(mem.read_u16(0x8000_001C), Some(0xC233));
    assert_eq!(mem.read_u32(0x8000_001C), Some(0xC233_9F3D));
    assert_eq!(mem.read_u32(0x8000_0020), Some(0x0D15_EA5E));
    assert_eq!(mem.read_f32(0x8000_001C), Some(f32::from_bits(0xC233_9F3D)));

    // address masking: 0x0-prefixed and 0x8-prefixed hit the same offset
    assert_eq!(mem.read_u32(0x0000_0000), mem.read_u32(0x8000_0000));
    assert_eq!(mem.read_u32(0x0000_001C), mem.read_u32(0x8000_001C));

    // out of range -> None (not a clamp to offset 0)
    assert_eq!(mem.read_u32(0x8190_0000), None);
    assert_eq!(mem.read_u32(0x8000_0000 + 0x17F_FFFE), None);
    assert_eq!(mem.read_u8(0x8000_0000 + 0x180_0000), None);

    // bitfield-masked reads
    assert_eq!(
      mem.read_u32_bits(0x8000_001C, 0, 0),
      mem.read_u32(0x8000_001C)
    );
    assert_eq!(mem.read_u32_bits(0x8000_001C, 4, 4), Some(0x3));
    assert_eq!(mem.read_u32_bits(0x8000_001C, 0, 8), Some(0x3D));
  }

  #[test]
  fn extract_bits_logic() {
    // (0xC2339F3D >> 4) & 0xF  and  & 0xFF
    assert_eq!(GameMemory::extract_bits(0xC233_9F3D, 4, 4, 32), 0x3);
    assert_eq!(GameMemory::extract_bits(0xC233_9F3D, 0, 8, 32), 0x3D);
    // bit_length == 0 -> full value
    assert_eq!(GameMemory::extract_bits(0xABCD, 0, 0, 32), 0xABCD);
    assert_eq!(GameMemory::extract_bits(0x00FF, 0, 0, 8), 0xFF);
    // bit past the value width -> 0 (guards `v >> 40` UB)
    assert_eq!(GameMemory::extract_bits(0xFFFF_FFFF, 40, 4, 32), 0);
    // bit_length >= width -> full mask (guards `1u64 << 64` UB via width_bits)
    assert_eq!(GameMemory::extract_bits(0xFF, 0, 64, 8), 0xFF);
    assert_eq!(GameMemory::extract_bits(u64::MAX, 0, 64, 64), u64::MAX);
  }

  #[test]
  fn signed_and_bool_reads() {
    let mut mem = blank();
    mem.data[0] = 0xFF;
    mem.data[1] = 0xFE;
    assert_eq!(mem.read_i8(0x8000_0000), Some(-1));
    assert_eq!(mem.read_i16(0x8000_0000), Some(-2));
    assert_eq!(mem.read_bool(0x8000_0000), Some(true));
    assert_eq!(mem.read_bool(0x8000_0002), Some(false));
    assert_eq!(mem.read_i32(0x8190_0000), None);

    // -2 as a full 64-bit two's-complement value
    mem.data[..8].copy_from_slice(&[0xFF; 8]);
    mem.data[7] = 0xFE;
    assert_eq!(mem.read_i64(0x8000_0000), Some(-2));
  }

  #[test]
  fn wide_and_bits_reads() {
    let mut mem = blank();
    mem.data[..8].copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]);
    assert_eq!(mem.read_u64(0x8000_0000), Some(0x1234_5678_9ABC_DEF0));
    assert_eq!(mem.read_i64(0x8000_0000), Some(0x1234_5678_9ABC_DEF0));
    assert_eq!(
      mem.read_f64(0x8000_0000),
      Some(f64::from_bits(0x1234_5678_9ABC_DEF0))
    );
    // bit == 0 && bit_length == 0 is identical to the plain read
    assert_eq!(
      mem.read_u8_bits(0x8000_0000, 0, 0),
      mem.read_u8(0x8000_0000)
    );
    assert_eq!(
      mem.read_u16_bits(0x8000_0000, 0, 0),
      mem.read_u16(0x8000_0000)
    );
    assert_eq!(
      mem.read_u64_bits(0x8000_0000, 0, 0),
      mem.read_u64(0x8000_0000)
    );
    // byte 0 = 0x12 = 0b0001_0010; (0x12 >> 1) & 0b111 = 0b001
    assert_eq!(mem.read_u8_bits(0x8000_0000, 1, 3), Some(0b001));
    // u16 value is 0x1234 (BE): low 8 bits = 0x34, next 8 bits = 0x12
    assert_eq!(mem.read_u16_bits(0x8000_0000, 0, 8), Some(0x34));
    assert_eq!(mem.read_u16_bits(0x8000_0000, 8, 8), Some(0x12));
    // u64 value 0x1234_5678_9ABC_DEF0: bits [32,48) = 0x5678
    assert_eq!(mem.read_u64_bits(0x8000_0000, 32, 16), Some(0x5678));
    assert_eq!(mem.read_u8_bits(0x8190_0000, 0, 0), None);
  }
}
