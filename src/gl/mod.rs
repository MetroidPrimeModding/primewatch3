//! Reusable wgpu building blocks.
//!
//! - [`Vert`] + its [`wgpu::VertexBufferLayout`].
//! - [`mesh::DynamicMesh`] — a growable vertex buffer.
//! - [`shader::WorldPipelines`] / [`shader::WorldUniforms`] — the world shader
//!   and its render pipelines.
//! - [`immediate::ImmediateModeBuffer`] (CPU-only).
//! - [`shapes`] — CPU-only procedural geometry.

pub mod immediate;
pub mod mesh;
pub mod shader;
pub mod shapes;

/// Packed interleaved vertex.
///
/// Every field is `f32`, so `#[repr(C)]` reproduces the C++ `packed` layout
/// byte-for-byte (there is no padding to pack away).
///
/// No `Default`: The `barycentric{-1,-1,-1}` in-class initializer is always
/// overwritten by [`crate::world::collision_mesh::CollisionMesh::build_vertices`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
  pub pos: [f32; 3],
  pub color: [f32; 4],
  pub normal: [f32; 3],
  pub barycentric: [f32; 3],
}

/// Vertex attributes — the four `glVertexAttribPointer` calls (location 0 `pos`
/// vec3, 1 `color` vec4, 2 `normal` vec3, 3 `barycentric` vec3). A `const` the
/// layout borrows.
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
/// egui-wgpu hard-requires a linear `Rgba8Unorm` texture for
/// `register_native_texture`, so the world pass renders into that (not the
/// surface's sRGB format) and does its own linear→sRGB in the shader.
pub const WORLD_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The world view's own depth target format — ports `glEnable(GL_DEPTH_TEST)`.
pub const WORLD_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Reinterpret a slice of plain-old-data as bytes for GPU upload. `u8` has
/// alignment 1 so any `[T]` is validly viewable as `[u8]`. (No `bytemuck` dep.)
pub(crate) fn as_bytes<T: Copy>(data: &[T]) -> &[u8] {
  // SAFETY: `T: Copy` POD, read-only view, size in bytes is exact.
  unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(data)) }
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
}
