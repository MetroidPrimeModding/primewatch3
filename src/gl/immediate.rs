//! `ImmediateModeBuffer`, **CPU-only**.
//!
//! **Deviation (sanctioned, for testability):** the C++ class owns two
//! `OpenGLMesh`es and does its own `drawTris` / `drawLines` upload+draw. This
//! port is pure CPU — it accumulates `Vec<Vert>` and exposes them; the
//! `WorldRenderer` owns the [`crate::gl::mesh::DynamicMesh`] pair and does
//! `mesh.upload(dev, q, imm.tri_verts()); mesh.draw(pass)` each frame. The
//! "push-vertices-per-frame" pattern is preserved, the GPU half is lifted to the
//! caller. So `drawTris` / `drawLines` become [`ImmediateModeBuffer::tri_verts`]
//! / [`ImmediateModeBuffer::line_verts`].

use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::gl::Vert;

const NO_BARYCENTRIC: [f32; 3] = [-1.0, -1.0, -1.0];

pub struct ImmediateModeBuffer {
  line_verts: Vec<Vert>,
  tri_verts: Vec<Vert>,
  current_color: [f32; 4],
  vert_transform: Mat4,
  normal_transform: Mat3,
}

impl Default for ImmediateModeBuffer {
  fn default() -> Self {
    Self::new()
  }
}

#[allow(dead_code)]
impl ImmediateModeBuffer {
  /// The ctor, minus the two `make_unique<OpenGLMesh>`.
  pub fn new() -> Self {
    Self {
      line_verts: Vec::new(),
      tri_verts: Vec::new(),
      current_color: [1.0, 1.0, 1.0, 1.0],
      vert_transform: Mat4::IDENTITY,
      normal_transform: Mat3::IDENTITY,
    }
  }

  /// `clear()`.
  pub fn clear(&mut self) {
    self.tri_verts.clear();
    self.line_verts.clear();
  }

  /// Accumulated triangle verts — replaces `drawTris`.
  pub fn tri_verts(&self) -> &[Vert] {
    &self.tri_verts
  }

  /// Accumulated line verts — replaces `drawLines`.
  pub fn line_verts(&self) -> &[Vert] {
    &self.line_verts
  }

  /// `setColor`.
  pub fn set_color(&mut self, color: [f32; 4]) {
    self.current_color = color;
  }

  /// `setTransform`: `normalTransform = transpose(inverse(vertTransform))`.
  pub fn set_transform(&mut self, tf: Mat4) {
    self.vert_transform = tf;
    self.normal_transform = Mat3::from_mat4(tf).inverse().transpose();
  }

  /// `addLine(vec3, vec3)`.
  pub fn add_line(&mut self, start: Vec3, end: Vec3) {
    let a = Vert {
      pos: start.to_array(),
      color: self.current_color,
      normal: [0.0, 0.0, 1.0],
      barycentric: NO_BARYCENTRIC,
    };
    let b = Vert {
      pos: end.to_array(),
      color: self.current_color,
      normal: [0.0, 0.0, 1.0],
      barycentric: NO_BARYCENTRIC,
    };
    self.add_line_vert(a, b);
  }

  /// `addLine(Vert, Vert)`.
  pub fn add_line_vert(&mut self, a: Vert, b: Vert) {
    self.add_lines(&[a, b]);
  }

  /// `addTri(vec3, vec3, vec3)`.
  pub fn add_tri(&mut self, a: Vec3, b: Vec3, c: Vec3) {
    let n = (a - b).cross(a - c).normalize();
    let mk = |pos: Vec3, bary: [f32; 3]| Vert {
      pos: pos.to_array(),
      color: self.current_color,
      normal: n.to_array(),
      barycentric: bary,
    };
    let va = mk(a, [1.0, 0.0, 0.0]);
    let vb = mk(b, [0.0, 1.0, 0.0]);
    let vc = mk(c, [0.0, 0.0, 1.0]);
    self.add_tri_vert(va, vb, vc);
  }

  /// `addTri(Vert, Vert, Vert)`.
  pub fn add_tri_vert(&mut self, a: Vert, b: Vert, c: Vert) {
    self.add_tris(&[a, b, c]);
  }

  /// `addQuad(vec3, vec3, vec3, vec3)` — note `d` reuses the `[1, 0, 0]`
  /// barycentric.
  pub fn add_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    let n = (a - b).cross(a - c).normalize();
    let mk = |pos: Vec3, bary: [f32; 3]| Vert {
      pos: pos.to_array(),
      color: self.current_color,
      normal: n.to_array(),
      barycentric: bary,
    };
    let va = mk(a, [1.0, 0.0, 0.0]);
    let vb = mk(b, [0.0, 1.0, 0.0]);
    let vc = mk(c, [0.0, 0.0, 1.0]);
    let vd = mk(d, [1.0, 0.0, 0.0]);
    self.add_quad_vert(va, vb, vc, vd);
  }

  /// `addQuad(Vert, Vert, Vert, Vert)`.
  pub fn add_quad_vert(&mut self, a: Vert, b: Vert, c: Vert, d: Vert) {
    self.add_tris(&[a, b, c, c, d, a]);
  }

  /// `addLines`: push a transformed copy of each input.
  pub fn add_lines(&mut self, verts: &[Vert]) {
    let vt = self.vert_transform;
    let nt = self.normal_transform;
    self
      .line_verts
      .extend(verts.iter().map(|v| transform_vert(vt, nt, v)));
  }

  /// `addTris`: push a transformed copy of each input.
  pub fn add_tris(&mut self, verts: &[Vert]) {
    let vt = self.vert_transform;
    let nt = self.normal_transform;
    self
      .tri_verts
      .extend(verts.iter().map(|v| transform_vert(vt, nt, v)));
  }
}

/// The per-vertex transform:
/// `pos = (vertTransform * vec4(pos, 1)).xyz`,
/// `normal = normalize(normalTransform * normal)`, colour + barycentric passed
/// through.
fn transform_vert(vt: Mat4, nt: Mat3, v: &Vert) -> Vert {
  let pos = (vt * Vec4::new(v.pos[0], v.pos[1], v.pos[2], 1.0)).truncate();
  let normal = (nt * Vec3::from_array(v.normal)).normalize();
  Vert {
    pos: pos.to_array(),
    color: v.color,
    normal: normal.to_array(),
    barycentric: v.barycentric,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn approx(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
  }

  #[test]
  fn default_color_is_white() {
    let imm = ImmediateModeBuffer::default();
    assert_eq!(imm.current_color, [1.0, 1.0, 1.0, 1.0]);
    assert!(imm.tri_verts().is_empty());
    assert!(imm.line_verts().is_empty());
  }

  #[test]
  fn set_transform_translation_leaves_normal_transform_identity() {
    let mut imm = ImmediateModeBuffer::new();
    imm.set_transform(Mat4::from_translation(Vec3::new(3.0, -2.0, 7.0)));
    let diff = imm.normal_transform - Mat3::IDENTITY;
    assert!(diff.to_cols_array().iter().all(|x| x.abs() < 1e-6));
  }

  #[test]
  fn set_transform_non_uniform_scale_is_inverse_transpose() {
    let mut imm = ImmediateModeBuffer::new();
    let s = Vec3::new(2.0, 4.0, 8.0);
    imm.set_transform(Mat4::from_scale(s));
    let expected = Mat3::from_mat4(Mat4::from_scale(s)).inverse().transpose();
    assert!(
      (imm.normal_transform - expected)
        .to_cols_array()
        .iter()
        .all(|x| x.abs() < 1e-6)
    );
    assert!((imm.normal_transform.x_axis.x - 0.5).abs() < 1e-6);
    assert!((imm.normal_transform.y_axis.y - 0.25).abs() < 1e-6);
    assert!((imm.normal_transform.z_axis.z - 0.125).abs() < 1e-6);
  }

  #[test]
  fn add_tri_ccw_in_z_plane_normal_and_barycentrics() {
    let mut imm = ImmediateModeBuffer::new();
    let a = Vec3::new(0.0, 0.0, 0.0);
    let b = Vec3::new(1.0, 0.0, 0.0);
    let c = Vec3::new(0.0, 1.0, 0.0);
    imm.add_tri(a, b, c);
    let v = imm.tri_verts();
    assert_eq!(v.len(), 3);
    // n = normalize(cross(a - b, a - c)) = normalize(cross((-1,0,0),(0,-1,0)))
    //   = normalize((0,0,1)) = (0,0,1)
    assert!(approx(v[0].normal, [0.0, 0.0, 1.0]));
    assert!(approx(v[1].normal, [0.0, 0.0, 1.0]));
    assert!(approx(v[2].normal, [0.0, 0.0, 1.0]));
    assert_eq!(v[0].barycentric, [1.0, 0.0, 0.0]);
    assert_eq!(v[1].barycentric, [0.0, 1.0, 0.0]);
    assert_eq!(v[2].barycentric, [0.0, 0.0, 1.0]);
  }

  #[test]
  fn add_quad_pushes_six_tri_verts_in_abccda_order() {
    let mut imm = ImmediateModeBuffer::new();
    let a = Vec3::new(0.0, 0.0, 0.0);
    let b = Vec3::new(1.0, 0.0, 0.0);
    let c = Vec3::new(1.0, 1.0, 0.0);
    let d = Vec3::new(0.0, 1.0, 0.0);
    imm.add_quad(a, b, c, d);
    let v = imm.tri_verts();
    assert_eq!(v.len(), 6);
    assert_eq!(v[0].pos, a.to_array());
    assert_eq!(v[1].pos, b.to_array());
    assert_eq!(v[2].pos, c.to_array());
    assert_eq!(v[3].pos, c.to_array());
    assert_eq!(v[4].pos, d.to_array());
    assert_eq!(v[5].pos, a.to_array());
  }

  #[test]
  fn add_tris_applies_vert_transform_to_positions() {
    let mut imm = ImmediateModeBuffer::new();
    imm.set_transform(Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)));
    imm.add_tri(
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(1.0, 0.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
    );
    let v = imm.tri_verts();
    assert!(approx(v[0].pos, [10.0, 0.0, 0.0]));
    assert!(approx(v[1].pos, [11.0, 0.0, 0.0]));
    assert!(approx(v[2].pos, [10.0, 1.0, 0.0]));
  }

  #[test]
  fn add_line_uses_no_barycentric_sentinel() {
    let mut imm = ImmediateModeBuffer::new();
    imm.add_line(Vec3::ZERO, Vec3::X);
    let v = imm.line_verts();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].barycentric, NO_BARYCENTRIC);
    assert_eq!(v[0].normal, [0.0, 0.0, 1.0]);
  }

  #[test]
  fn clear_empties_both() {
    let mut imm = ImmediateModeBuffer::new();
    imm.add_line(Vec3::ZERO, Vec3::X);
    imm.add_tri(Vec3::ZERO, Vec3::X, Vec3::Y);
    imm.clear();
    assert!(imm.tri_verts().is_empty());
    assert!(imm.line_verts().is_empty());
  }
}
