//! glam readers for the game's math structs.
//!
//! Each reader takes the live `GameInstance` for the member (address + type)
//! and unpacks its floats into a glam type.

use crate::ctx::Ctx;
use crate::structs::prime_structs::GameInstance;
use glam::{Mat4, Quat, Vec3};

/// `member["x" / "y" / "z"]` as three f32s.
pub fn read_as_vec3(ctx: &Ctx, member: &GameInstance) -> Option<Vec3> {
  Some(Vec3::new(
    member.get_member(ctx, "x")?.read_f32(ctx)?,
    member.get_member(ctx, "y")?.read_f32(ctx)?,
    member.get_member(ctx, "z")?.read_f32(ctx)?,
  ))
}

/// `member["x" / "y" / "z" / "w"]` as four f32s, in `(x, y, z, w)` order
pub fn read_as_quat(ctx: &Ctx, member: &GameInstance) -> Option<Quat> {
  Some(Quat::from_xyzw(
    member.get_member(ctx, "x")?.read_f32(ctx)?,
    member.get_member(ctx, "y")?.read_f32(ctx)?,
    member.get_member(ctx, "z")?.read_f32(ctx)?,
    member.get_member(ctx, "w")?.read_f32(ctx)?,
  ))
}

/// 16 raw floats off `member["matrix"]` with the `RC(r, c) = (r + c * 4) * 4`
/// byte offset. The 16 values are handed to `Mat4::from_cols_array`
/// in the *same* order: the first four form column 0, the next
/// four column 1, and so on — both constructors are column-major, so the byte
/// layout matches C++ exactly.
pub fn read_as_matrix4f(ctx: &Ctx, member: &GameInstance) -> Option<Mat4> {
  let base = member.get_member(ctx, "matrix")?.address;
  let rc = |r: u32, c: u32| ctx.mem.read_f32(base.wrapping_add((r + c * 4) * 4));
  Some(Mat4::from_cols_array(&[
    rc(0, 0)?,
    rc(0, 1)?,
    rc(0, 2)?,
    rc(0, 3)?,
    rc(1, 0)?,
    rc(1, 1)?,
    rc(1, 2)?,
    rc(1, 3)?,
    rc(2, 0)?,
    rc(2, 1)?,
    rc(2, 2)?,
    rc(2, 3)?,
    rc(3, 0)?,
    rc(3, 1)?,
    rc(3, 2)?,
    rc(3, 3)?,
  ]))
}

/// 12 raw floats off `member["m0"]` with the same `RC(r, c) = (r + c * 4) * 4`
/// offset, with the fourth element of the first three columns hardcoded `0.0`
/// and the fourth column's `w` hardcoded `1.0` — `w` is *not* read from memory.
pub fn read_as_transform(ctx: &Ctx, member: &GameInstance) -> Option<Mat4> {
  let base = member.get_member(ctx, "m0")?.address;
  let rc = |r: u32, c: u32| ctx.mem.read_f32(base.wrapping_add((r + c * 4) * 4));
  Some(Mat4::from_cols_array(&[
    rc(0, 0)?,
    rc(0, 1)?,
    rc(0, 2)?,
    0.0,
    rc(1, 0)?,
    rc(1, 1)?,
    rc(1, 2)?,
    0.0,
    rc(2, 0)?,
    rc(2, 1)?,
    rc(2, 2)?,
    0.0,
    rc(3, 0)?,
    rc(3, 1)?,
    rc(3, 2)?,
    1.0,
  ]))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::game_memory::GameMemory;
  use crate::structs::prime_structs::{GameMember, GameStruct, GameStructs};

  fn member_def(name: &str, type_name: &str, offset: i64) -> GameMember {
    GameMember {
      type_name: type_name.to_string(),
      name: name.to_string(),
      offset,
      bit: None,
      bit_length: None,
      array_length: None,
      pointer: false,
    }
  }

  fn game_struct(name: &str, members: &[GameMember]) -> GameStruct {
    let mut s = GameStruct {
      name: name.to_string(),
      size: 0,
      vtable_address: None,
      extends: vec![],
      members_by_offset: Default::default(),
      members_by_name: Default::default(),
    };
    for m in members {
      s.insert_member(m);
    }
    s
  }

  /// The three math structs the readers traverse. `f32` array elements are read
  /// straight off the resolved address, so `CMatrix4f` only needs a `matrix`
  /// member by name and `CTransform` only an `m0` member by name.
  fn math_structs() -> GameStructs {
    let mut s = GameStructs::new_empty();
    s.insert_struct(&game_struct(
      "CVector3f",
      &[
        member_def("x", "f32", 0x0),
        member_def("y", "f32", 0x4),
        member_def("z", "f32", 0x8),
      ],
    ));
    s.insert_struct(&game_struct(
      "CQuaternion",
      &[
        member_def("x", "f32", 0x0),
        member_def("y", "f32", 0x4),
        member_def("z", "f32", 0x8),
        member_def("w", "f32", 0xC),
      ],
    ));
    s.insert_struct(&game_struct(
      "CMatrix4f",
      &[member_def("matrix", "f32", 0x0)],
    ));
    s.insert_struct(&game_struct(
      "CTransform",
      &[member_def("m0", "CVector3f", 0x0)],
    ));
    s
  }

  fn mem_with_floats(base: u32, floats: &[f32]) -> GameMemory {
    let mut mem = GameMemory::new();
    let off = (base & 0x7FFF_FFFF) as usize;
    for (i, v) in floats.iter().enumerate() {
      mem.data[off + i * 4..off + i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    mem
  }

  const BASE: u32 = 0x8010_0000;

  #[test]
  fn vec3_unpacks_xyz() {
    let structs = math_structs();
    let mem = mem_with_floats(BASE, &[1.5, -2.25, 100.0]);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(BASE, "CVector3f".to_string());
    assert_eq!(
      read_as_vec3(&ctx, &inst),
      Some(Vec3::new(1.5, -2.25, 100.0))
    );
  }

  #[test]
  fn quat_unpacks_xyzw_in_order() {
    let structs = math_structs();
    let mem = mem_with_floats(BASE, &[0.1, 0.2, 0.3, 0.9]);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(BASE, "CQuaternion".to_string());
    let q = read_as_quat(&ctx, &inst).unwrap();
    assert_eq!([q.x, q.y, q.z, q.w], [0.1, 0.2, 0.3, 0.9]);
  }

  #[test]
  fn matrix4f_is_column_major_like_cpp() {
    // 16 distinct floats laid out linearly at BASE.
    let raw: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let structs = math_structs();
    let mem = mem_with_floats(BASE, &raw);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(BASE, "CMatrix4f".to_string());
    let m = read_as_matrix4f(&ctx, &inst).unwrap();

    // C++ `glm::mat4(RC(0,0), RC(0,1), RC(0,2), RC(0,3), RC(1,0), ...)` puts the
    // first four args in column 0. RC(r,c) = (r + c*4)*4 bytes => raw index
    // r + c*4. So column 0 = raw[0], raw[4], raw[8], raw[12].
    let cols = m.to_cols_array();
    for c in 0..4usize {
      for r in 0..4usize {
        // glam column-major array index = c*4 + r; C++ arg position = c*4 + r
        // and that arg is RC(c, r) => raw[c + r*4].
        assert_eq!(cols[c * 4 + r], raw[c + r * 4], "cell ({r},{c})");
      }
    }
  }

  #[test]
  fn transform_hardcodes_last_column_and_zero_rows() {
    let raw: Vec<f32> = (0..16).map(|i| i as f32 + 0.5).collect();
    let structs = math_structs();
    let mem = mem_with_floats(BASE, &raw);
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(BASE, "CTransform".to_string());
    let m = read_as_transform(&ctx, &inst).unwrap();
    let cols = m.to_cols_array();

    // Columns 0..3: [raw[c], raw[c+4], raw[c+8], 0.0]
    for c in 0..3usize {
      assert_eq!(cols[c * 4], raw[c]);
      assert_eq!(cols[c * 4 + 1], raw[c + 4]);
      assert_eq!(cols[c * 4 + 2], raw[c + 8]);
      assert_eq!(cols[c * 4 + 3], 0.0);
    }
    // Column 3 (translation): [raw[3], raw[7], raw[11], 1.0]
    assert_eq!(cols[12], raw[3]);
    assert_eq!(cols[13], raw[7]);
    assert_eq!(cols[14], raw[11]);
    assert_eq!(cols[15], 1.0);
  }

  #[test]
  fn missing_member_yields_none() {
    let structs = math_structs();
    let mem = mem_with_floats(BASE, &[1.0, 2.0, 3.0]);
    let ctx = Ctx::new(&structs, &mem);
    // Instance typed as a struct with no x/y/z members.
    let inst = GameInstance::new(BASE, "CMatrix4f".to_string());
    assert_eq!(read_as_vec3(&ctx, &inst), None);
  }

  #[test]
  fn oob_address_yields_none() {
    let structs = math_structs();
    let mem = GameMemory::new();
    let ctx = Ctx::new(&structs, &mem);
    let inst = GameInstance::new(0x81F0_0000, "CTransform".to_string());
    assert_eq!(read_as_transform(&ctx, &inst), None);
  }

  /// Skip-if-absent loader for the offline BE dump — same contract as the
  /// `game_memory.rs` / `prime_structs.rs` tests.
  fn load_mem1() -> Option<GameMemory> {
    let path = std::env::var("PRIMEWATCH_MEM1_RAW")
      .unwrap_or_else(|_| format!("{}/mem1.raw", env!("CARGO_MANIFEST_DIR")));
    if !std::path::Path::new(&path).exists() {
      eprintln!("skipping math_utils mem1.raw test: {path} not found");
      return None;
    }
    let mut mem = GameMemory::new();
    mem.load_from_file(&path).expect("read mem1.raw");
    Some(mem)
  }

  /// Byte-layout check against the real BE dump: treat the disc header as a raw
  /// float block and confirm every value the reader emits matches a direct
  /// `GameMemory::read_f32` at the C++ `RC` offset. `Mat4::to_cols_array()`
  /// index `p` holds the value C++ passes as `glm::mat4` arg `p`, i.e.
  /// `RC(p / 4, p % 4) = ((p / 4) + (p % 4) * 4) * 4` bytes.
  #[test]
  fn matrix_offsets_match_raw_dump() {
    let Some(mem) = load_mem1() else {
      return;
    };
    let structs = math_structs();
    let ctx = Ctx::new(&structs, &mem);
    let base: u32 = 0x8000_0000;
    let inst = GameInstance::new(base, "CMatrix4f".to_string());
    let m = read_as_matrix4f(&ctx, &inst).unwrap().to_cols_array();
    for (p, &got) in m.iter().enumerate() {
      let p = p as u32;
      let expected = mem.read_f32(base + (p / 4 + (p % 4) * 4) * 4).unwrap();
      assert!(
        got == expected || (got.is_nan() && expected.is_nan()),
        "arg {p}"
      );
    }
  }
}
