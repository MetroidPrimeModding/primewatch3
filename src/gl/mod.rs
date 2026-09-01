//! Reusable wgpu building blocks — ports `../primewatch2/src/gl/*` (everything
//! except `ShapeGenerator`, which is P8.3).
//!
//! - [`Vert`] + its [`wgpu::VertexBufferLayout`] — ports `gl/OpenGLMesh.hpp` +
//!   the four `glVertexAttribPointer` calls in `gl/OpenGLMesh.cpp:14-21`.
//! - [`mesh::DynamicMesh`] — ports `OpenGLMesh`.
//! - [`shader::WorldPipelines`] / [`shader::WorldUniforms`] — ports
//!   `OpenGLShader` + the three GLSL shader strings in `WorldRenderer.cpp:31-113`.
//! - [`immediate::ImmediateModeBuffer`] — ports `ImmediateModeBuffer` (CPU-only).
//!
//! Everything here is library code: dead until P8.4 (`WorldRenderer`) wires it.

pub mod immediate;
pub mod mesh;
pub mod shader;

/// Packed interleaved vertex — ports `gl/OpenGLMesh.hpp:12-17` `Vert`.
///
/// Every field is `f32`, so `#[repr(C)]` reproduces the C++ `packed` layout
/// byte-for-byte (there is no padding to pack away).
///
/// No `Default`: the C++ `barycentric{-1,-1,-1}` in-class initializer is always
/// overwritten by [`crate::world::collision_mesh::CollisionMesh::build_vertices`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
  pub pos: [f32; 3],
  pub color: [f32; 4],
  pub normal: [f32; 3],
  pub barycentric: [f32; 3],
}

/// Vertex attributes — ports the four `glVertexAttribPointer` calls in
/// `gl/OpenGLMesh.cpp:14-21` (location 0 `pos` vec3, 1 `color` vec4, 2 `normal`
/// vec3, 3 `barycentric` vec3). A `const` the layout borrows — same pattern as
/// `scene.rs::VERTEX_ATTRS`.
const VERTEX_ATTRS: [wgpu::VertexAttribute; 4] =
  wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x3, 3 => Float32x3];

impl Vert {
  /// Interleaved layout for [`Vert`]. `array_stride == size_of::<Vert>() == 52`
  /// (a valid vertex stride — multiple of 4). No padding: don't pad `Vert`.
  pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<Vert>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRS,
  };
}

/// The world view's offscreen colour target format.
///
/// P1.3 decision: egui-wgpu hard-requires a linear `Rgba8Unorm` texture for
/// `register_native_texture`, so the world pass renders into that (not the
/// surface's sRGB format) and does its own linear→sRGB in the shader. Same value
/// as the private `COLOR_FORMAT` in `scene.rs`; P8.4 unifies the copies.
pub const WORLD_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The world view's own depth target format — ports `glEnable(GL_DEPTH_TEST)`.
/// Same value as the private `DEPTH_FORMAT` in `scene.rs`; P8.4 unifies.
pub const WORLD_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Reinterpret a slice of plain-old-data as bytes for GPU upload. `u8` has
/// alignment 1 so any `[T]` is validly viewable as `[u8]`. Copied verbatim from
/// `scene.rs::as_bytes` (P1.3 decision: no `bytemuck` dep).
pub(crate) fn as_bytes<T: Copy>(data: &[T]) -> &[u8] {
  // SAFETY: `T: Copy` POD, read-only view, size in bytes is exact.
  unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(data)) }
}

/// Primitive topology — ports the used subset of `gl/OpenGLMesh.hpp:22-30`
/// `RenderType`. The other five GL modes (`POINTS`, `LINE_LOOP`, `LINE_STRIP`,
/// `TRIANGLE_STRIP`, `TRIANGLE_FAN`) are unused in the codebase, and
/// `LINE_LOOP` / `TRIANGLE_FAN` have no wgpu-core equivalent.
///
/// `BufferUpdateType` (`STATIC` / `DYNAMIC` / `STREAM`) is dropped: a wgpu
/// dynamic buffer is just `VERTEX | COPY_DST` + `queue.write_buffer`, the GL
/// usage hint has no analogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topology {
  Lines,
  Triangles,
}

impl Topology {
  pub fn to_wgpu(self) -> wgpu::PrimitiveTopology {
    match self {
      Topology::Lines => wgpu::PrimitiveTopology::LineList,
      Topology::Triangles => wgpu::PrimitiveTopology::TriangleList,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn vert_layout_matches_packed_cpp_struct() {
    assert_eq!(std::mem::size_of::<Vert>(), 52);
    assert_eq!(Vert::LAYOUT.array_stride, 52);
    assert_eq!(Vert::LAYOUT.attributes.len(), 4);
    // Offsets: pos @0, color @12, normal @28, barycentric @40.
    assert_eq!(Vert::LAYOUT.attributes[0].offset, 0);
    assert_eq!(Vert::LAYOUT.attributes[1].offset, 12);
    assert_eq!(Vert::LAYOUT.attributes[2].offset, 28);
    assert_eq!(Vert::LAYOUT.attributes[3].offset, 40);
  }

  #[test]
  fn topology_maps_to_wgpu() {
    assert_eq!(Topology::Lines.to_wgpu(), wgpu::PrimitiveTopology::LineList);
    assert_eq!(
      Topology::Triangles.to_wgpu(),
      wgpu::PrimitiveTopology::TriangleList
    );
  }
}
