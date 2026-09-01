//! CPU-side world-geometry data layer — ports `../primewatch2/src/world/*` and
//! the vertex type from `../primewatch2/src/gl/*`.
//!
//! Scope for P8.1 is the collision-geometry parse + triangle-soup build only.
//! wgpu upload / pipelines (`OpenGLMesh`, `draw()`) land in P8.2; the
//! `mesh_by_mrea` cache + `updateAreas` maintenance land in P8.4.

pub mod collision_mesh;

/// Packed interleaved vertex — ports `gl/OpenGLMesh.hpp:12-17` `Vert`.
///
/// Every field is `f32`, so `#[repr(C)]` reproduces the C++ `packed` layout
/// byte-for-byte (there is no padding to pack away). P8.2 adds the
/// `wgpu::VertexBufferLayout` for this type.
///
/// No `Default`: the C++ `barycentric{-1,-1,-1}` in-class initializer is always
/// overwritten by [`collision_mesh::CollisionMesh::build_vertices`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
  pub pos: [f32; 3],
  pub color: [f32; 4],
  pub normal: [f32; 3],
  pub barycentric: [f32; 3],
}
