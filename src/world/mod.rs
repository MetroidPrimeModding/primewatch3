//! CPU-side world-geometry data layer — ports `../primewatch2/src/world/*`.
//!
//! Scope for P8.1 is the collision-geometry parse + triangle-soup build only.
//! The vertex type ([`crate::gl::Vert`]) and wgpu upload / pipelines live in
//! `crate::gl` (P8.2); the `mesh_by_mrea` cache + `updateAreas` maintenance land
//! in P8.4.

pub mod collision_mesh;
pub mod renderer;
