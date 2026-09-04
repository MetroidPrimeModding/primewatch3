//! GPU-facing plumbing: offscreen target creation, the `mesh_by_mrea` /
//! `gpu_mesh_by_mrea` collision-mesh cache (`WorldRenderer::updateAreas`), and
//! the render pass itself (`WorldRenderer::render`, minus `renderEntities`).

use glam::{Mat4, Vec4};

use crate::ctx::Ctx;
use crate::gl::mesh::DynamicMesh;
use crate::gl::shader::WorldUniforms;
use crate::gl::{WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT, shapes};
use crate::mem::area_utils::get_areas;
use crate::world::collision_mesh::{CollisionMesh, load_mesh};

use super::WorldRenderer;
use super::camera::orbit_z_nudge;
use super::types::OrbitPlayerCameraOrigin;

pub(super) fn clamp_size(size: (u32, u32)) -> (u32, u32) {
  (size.0.max(1), size.1.max(1))
}

/// Build the offscreen colour + depth targets. Uses the shared
/// `gl::WORLD_*_FORMAT` consts.
pub(super) fn create_targets(
  device: &wgpu::Device,
  size: (u32, u32),
) -> (wgpu::TextureView, wgpu::TextureView) {
  let extent = wgpu::Extent3d {
    width: size.0,
    height: size.1,
    depth_or_array_layers: 1,
  };
  let color = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("world-color"),
    size: extent,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: WORLD_COLOR_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let depth = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("world-depth"),
    size: extent,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: WORLD_DEPTH_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    view_formats: &[],
  });
  (
    color.create_view(&wgpu::TextureViewDescriptor::default()),
    depth.create_view(&wgpu::TextureViewDescriptor::default()),
  )
}

/// Pure `mesh_by_mrea` / GPU-cache bookkeeping for one area, factored out of
/// [`WorldRenderer::update_areas`] so it's testable without a GPU device.
/// `loaded` = `isPostConstructed`; `load` produces the CPU mesh on a cache miss.
fn reconcile_area<F: FnOnce() -> Option<CollisionMesh>>(
  mesh_by_mrea: &mut std::collections::HashMap<u32, CollisionMesh>,
  gpu_mesh_by_mrea: &mut std::collections::HashMap<u32, DynamicMesh>,
  mrea: u32,
  loaded: bool,
  load: F,
) {
  if !loaded {
    mesh_by_mrea.remove(&mrea);
    gpu_mesh_by_mrea.remove(&mrea);
    return;
  }
  if mesh_by_mrea.contains_key(&mrea) {
    return;
  }
  if let Some(m) = load() {
    mesh_by_mrea.insert(mrea, m);
  }
}

impl WorldRenderer {
  /// `WorldRenderer::updateAreas`.
  pub(super) fn update_areas(&mut self, ctx: &Ctx) {
    for area in get_areas(ctx) {
      let Some(mrea) = area.get_member(ctx, "mrea").and_then(|m| m.read_u32(ctx)) else {
        continue;
      };
      let Some(loaded) = area
        .get_member(ctx, "isPostConstructed")
        .and_then(|m| m.read_bool(ctx))
      else {
        continue;
      };
      reconcile_area(
        &mut self.mesh_by_mrea,
        &mut self.gpu_mesh_by_mrea,
        mrea,
        loaded,
        || load_mesh(ctx, &area),
      );
    }
  }

  /// The GPU half of `WorldRenderer::render` (minus `renderEntities`). Adds one
  /// render pass into the offscreen `(color, depth)` target.
  pub fn render(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
  ) {
    // Sync GPU collision meshes to the CPU cache.
    let want: Vec<u32> = self.mesh_by_mrea.keys().copied().collect();
    for mrea in want {
      if !self.gpu_mesh_by_mrea.contains_key(&mrea) {
        let mut dm = DynamicMesh::new(device, "collision-mesh");
        dm.upload(device, queue, &self.mesh_by_mrea[&mrea].verts);
        self.gpu_mesh_by_mrea.insert(mrea, dm);
      }
    }
    self
      .gpu_mesh_by_mrea
      .retain(|k, _| self.mesh_by_mrea.contains_key(k));

    // Per-mesh AABB wireframe boxes — done here so
    // it happens after `update_areas` regardless of call ordering.
    self.render_buff.set_transform(Mat4::IDENTITY);
    for mesh in self.mesh_by_mrea.values() {
      self
        .render_buff
        .add_lines(&shapes::generate_cube_lines(mesh.min, mesh.max, Vec4::ONE));
    }

    // Upload the two immediate buffers into the four dynamic meshes.
    self
      .opaque_tris
      .upload(device, queue, self.render_buff.tri_verts());
    self
      .opaque_lines
      .upload(device, queue, self.render_buff.line_verts());
    self
      .translucent_tris
      .upload(device, queue, self.translucent_render_buff.tri_verts());
    self
      .translucent_lines
      .upload(device, queue, self.translucent_render_buff.line_verts());
    self
      .player_tris
      .upload(device, queue, self.player_render_buff.tri_verts());
    self.player_translucent_tris.upload(
      device,
      queue,
      self.player_translucent_render_buff.tri_verts(),
    );

    let mut player_pos = self.last_known_non_colliding_pos;
    player_pos.z += orbit_z_nudge(OrbitPlayerCameraOrigin::Center, self.player.is_morphed);

    // model is identity for every draw: the immediate buffers bake per-vertex
    // transforms and collision verts are already world-space.
    let mut uniforms = WorldUniforms::from_matrices(
      Mat4::IDENTITY,
      self.cam_view,
      self.cam_projection,
      self.cam_eye,
      self.light_dir.normalize(),
      player_pos,
    );
    let clip = &self.player_clip_config;
    uniforms.clip_params = Vec4::new(
      clip.cone_radius,
      clip.player_margin,
      clip.player_fade,
      if clip.enabled { 1.0 } else { 0.0 },
    );
    uniforms.clip_params2 = Vec4::new(clip.min_visibility, 0.0, 0.0, 0.0);
    self.pipelines.set_uniforms(queue, &uniforms);

    let mesh_cull = match self.culling {
      super::types::CullType::Back => Some(wgpu::Face::Back),
      super::types::CullType::Front => Some(wgpu::Face::Front),
      super::types::CullType::None => None,
    };

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("world-pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: &self.color,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
        view: &self.depth,
        depth_ops: Some(wgpu::Operations {
          load: wgpu::LoadOp::Clear(1.0),
          store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
      }),
      timestamp_writes: None,
      occlusion_query_set: None,
      multiview_mask: None,
    });

    pass.set_bind_group(0, &self.pipelines.bind_group, &[]);

    // (a) collision meshes — honour `self.culling`.
    pass.set_pipeline(self.pipelines.mesh(false, mesh_cull));
    for dm in self.gpu_mesh_by_mrea.values() {
      dm.draw(&mut pass);
    }

    // (b) opaque immediate buffer — tris always back-culled, lines never culled.
    pass.set_pipeline(self.pipelines.mesh(false, Some(wgpu::Face::Back)));
    self.opaque_tris.draw(&mut pass);
    pass.set_pipeline(&self.pipelines.line_opaque);
    self.opaque_lines.draw(&mut pass);

    // (b') opaque player / ghost models — never clipped by the near-player cutout.
    pass.set_bind_group(0, &self.pipelines.bind_group_noclip, &[]);
    pass.set_pipeline(self.pipelines.mesh(false, Some(wgpu::Face::Back)));
    self.player_tris.draw(&mut pass);
    pass.set_bind_group(0, &self.pipelines.bind_group, &[]);

    // (c) translucent immediate buffer.
    pass.set_pipeline(self.pipelines.mesh(true, Some(wgpu::Face::Back)));
    self.translucent_tris.draw(&mut pass);
    pass.set_pipeline(&self.pipelines.line_translucent);
    self.translucent_lines.draw(&mut pass);

    // (c') translucent player / ghost models — never clipped.
    pass.set_bind_group(0, &self.pipelines.bind_group_noclip, &[]);
    pass.set_pipeline(self.pipelines.mesh(true, Some(wgpu::Face::Back)));
    self.player_translucent_tris.draw(&mut pass);
    pass.set_bind_group(0, &self.pipelines.bind_group, &[]);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reconcile_area_adds_then_evicts() {
    let mut cpu: std::collections::HashMap<u32, CollisionMesh> = std::collections::HashMap::new();
    let mut gpu: std::collections::HashMap<u32, DynamicMesh> = std::collections::HashMap::new();

    reconcile_area(&mut cpu, &mut gpu, 0x11, true, || {
      Some(CollisionMesh::default())
    });
    assert!(cpu.contains_key(&0x11));

    // Second post-constructed pass must not reload (closure would panic).
    reconcile_area(&mut cpu, &mut gpu, 0x11, true, || {
      panic!("should not reload")
    });
    assert!(cpu.contains_key(&0x11));

    // No longer post-constructed -> evicted from both caches.
    reconcile_area(&mut cpu, &mut gpu, 0x11, false, || None);
    assert!(!cpu.contains_key(&0x11));
    assert!(!gpu.contains_key(&0x11));
  }

  #[test]
  fn reconcile_area_load_failure_leaves_cache_empty() {
    let mut cpu: std::collections::HashMap<u32, CollisionMesh> = std::collections::HashMap::new();
    let mut gpu: std::collections::HashMap<u32, DynamicMesh> = std::collections::HashMap::new();
    reconcile_area(&mut cpu, &mut gpu, 0x22, true, || None);
    assert!(cpu.is_empty());
  }
}
