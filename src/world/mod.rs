//! CPU-side world-geometry data layer: the collision-geometry parse, the
//! triangle-soup build, and the `WorldRenderer` that drives the live 3D world
//! view. The vertex type ([`crate::gl::Vert`]) and wgpu upload / pipelines live
//! in `crate::gl`.

pub mod collision_mesh;
pub mod renderer;
