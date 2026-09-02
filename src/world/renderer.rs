//! `WorldRenderer` — the live 3D world view. Ports the non-entity half of
//! `../primewatch2/src/world/WorldRenderer.{hpp,cpp}` (the `renderEntities` /
//! `drawPlayer` / per-class draw functions and the ImGui status windows are
//! P8.4.3+).
//!
//! Replaces the P1.3 `SpikeScene`: same offscreen-target contract
//! (`render` hands back a colour `TextureView` for egui to composite), now
//! driven by the game's memory — the three camera modes, the `mesh_by_mrea`
//! collision-mesh cache + GPU upload, and the area-AABB / camera-frustum line
//! overlays.
//!
//! Deviations from the C++ are called out at each site; the load-bearing ones:
//! - Camera reads keep the last good value on a `None` (mid-load) rather than
//!   zeroing — a zeroed transform would snap the camera to the origin every load.
//! - `fov` is passed to `perspective` unconverted, exactly as the C++ passes it
//!   to `glm::perspective` (see [`perspective`]).
//! - `glm::decompose` -> `cam_eye = cam_view.inverse().w_axis` (only `cam_eye`
//!   is consumed this phase).

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::mesh::DynamicMesh;
use crate::gl::shader::{WorldPipelines, WorldUniforms};
use crate::gl::{Topology, WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT, shapes};
use crate::mem::area_utils::get_areas;
use crate::mem::game_object_utils::get_object_by_entity_id;
use crate::mem::globals::get_state_manager;
use crate::mem::math_utils::{read_as_matrix4f, read_as_quat, read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::{CollisionMesh, load_mesh};

/// Ports `enum class CullType` (`WorldRenderer.hpp:19-23`). Variants other than
/// `Back` are selected by the P8.4.6 camera/render-config UI.
#[allow(dead_code)] // Front/None wired up by the P8.4.6 render-config UI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CullType {
  Back,
  Front,
  None,
}

/// Ports `enum class CameraMode` (`WorldRenderer.hpp:25-29`). `Detached` /
/// `GameCam` are selected by the P8.4.6 UI.
#[allow(dead_code)] // Detached/GameCam wired up by the P8.4.6 camera-mode UI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
  FollowPlayer,
  Detached,
  GameCam,
}

/// Ports `enum class OrbitPlayerCameraOrigin` (`WorldRenderer.hpp:31-35`).
/// `Top` / `Bottom` are selected by the P8.4.6 UI.
#[allow(dead_code)] // Top/Bottom wired up by the P8.4.6 camera-mode UI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrbitPlayerCameraOrigin {
  Top,
  Center,
  Bottom,
}

/// Ports `struct GameCamera` (`WorldRenderer.hpp:37-44`) — the in-game camera as
/// read from `CGameCamera`. Only `perspective` / `transform` are consumed this
/// phase; `fov` / `znear` / `zfar` / `aspect` are read for the P8.4.5 status UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameCamera {
  pub perspective: Mat4,
  pub transform: Mat4,
  // P8.4.5: shown by the camera status window
  pub fov: f32,

  pub znear: f32,

  pub zfar: f32,

  pub aspect: f32,
}

/// Ports `PrimeWatchInput` (`../primewatch2/src/PrimeWatchInput.hpp`) minus
/// `capturedMouse`. Real winit plumbing is P9.1; `app.rs` passes
/// [`WorldInput::default`] (all zero — no camera motion) for now.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldInput {
  pub cam_pitch: f32,
  pub cam_yaw: f32,
  pub cam_zoom: f32,
}

fn clamp_size(size: (u32, u32)) -> (u32, u32) {
  (size.0.max(1), size.1.max(1))
}

/// Build the offscreen colour + depth targets. Folded in from the deleted
/// `scene.rs::create_targets`; uses the shared `gl::WORLD_*_FORMAT` consts
/// (P8.2 forward-note "P8.4 unifies scene.rs's format consts").
fn create_targets(
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

/// The `lookPos.z += …` nudge in `WorldRenderer.cpp:263-281`.
fn orbit_z_nudge(origin: OrbitPlayerCameraOrigin, morphed: bool) -> f32 {
  match origin {
    OrbitPlayerCameraOrigin::Top => {
      if morphed {
        1.4
      } else {
        2.7
      }
    }
    OrbitPlayerCameraOrigin::Center => {
      if morphed {
        0.7
      } else {
        1.35
      }
    }
    OrbitPlayerCameraOrigin::Bottom => 0.0,
  }
}

/// Ports `glm::quat(glm::vec3 eulerAngle)` — glm's half-angle constructor
/// (`detail/type_quat.inl`):
/// ```text
/// c = cos(euler * 0.5); s = sin(euler * 0.5);
/// w = c.x*c.y*c.z + s.x*s.y*s.z
/// x = s.x*c.y*c.z - c.x*s.y*s.z
/// y = c.x*s.y*c.z + s.x*c.y*s.z
/// z = c.x*c.y*s.z - s.x*s.y*c.z
/// ```
/// `WorldRenderer.cpp` calls it as `glm::quat(glm::vec3(0, pitch, yaw))`, i.e.
/// `euler.x = 0`, `euler.y = pitch`, `euler.z = yaw`.
pub fn quat_from_euler(euler: Vec3) -> Quat {
  let h = euler * 0.5;
  let (sx, cx) = h.x.sin_cos();
  let (sy, cy) = h.y.sin_cos();
  let (sz, cz) = h.z.sin_cos();
  Quat::from_xyzw(
    sx * cy * cz - cx * sy * sz,
    cx * sy * cz + sx * cy * sz,
    cx * cy * sz - sx * sy * cz,
    cx * cy * cz + sx * sy * sz,
  )
}

/// Ports `glm::perspective(fov, aspect, zNear, zFar)` (`WorldRenderer.cpp:259` /
/// `291`).
///
/// NOTE: the C++ passes `fov` (default `45`) straight into `glm::perspective`,
/// whose first parameter is the vertical FOV in **radians** — `45` rad is
/// almost certainly a latent bug in the original. This is ported verbatim: no
/// degrees→radians conversion here. Flagged in the P8.4.2 manual checklist.
///
/// Uses glam's DirectX-convention RH projection ([0, 1] clip depth) — the wgpu
/// convention, same call the deleted `scene.rs` used.
fn perspective(fov: f32, aspect: f32, z_near: f32, z_far: f32) -> Mat4 {
  glam::camera::rh::proj::directx::perspective(fov, aspect.max(1e-3), z_near, z_far)
}

/// Pure inputs for [`compute_camera`] — the camera-relevant `WorldRenderer`
/// fields, copied so the math is unit-testable without a GPU device.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraParams {
  pub camera_mode: CameraMode,
  pub orbit: OrbitPlayerCameraOrigin,
  pub fov: f32,
  pub aspect: f32,
  pub z_near: f32,
  pub z_far: f32,
  pub pitch: f32,
  pub yaw: f32,
  pub distance: f32,
  pub up: Vec3,
  pub player_is_morphed: bool,
  pub last_known_non_colliding_pos: Vec3,
  pub manual_camera_pos: Vec3,
  pub game_cam: GameCamera,
}

pub(crate) struct CameraResult {
  pub projection: Mat4,
  pub view: Mat4,
  pub eye: Vec3,
  pub manual_camera_pos: Vec3,
}

/// Ports the camera-setup block of `WorldRenderer::render`
/// (`WorldRenderer.cpp:258-310`).
pub(crate) fn compute_camera(p: &CameraParams) -> CameraResult {
  let mut manual_camera_pos = p.manual_camera_pos;
  let (projection, view) = match p.camera_mode {
    CameraMode::FollowPlayer => {
      let proj = perspective(p.fov, p.aspect, p.z_near, p.z_far);
      let angle = quat_from_euler(Vec3::new(0.0, p.pitch, p.yaw));
      let mut look_pos = p.last_known_non_colliding_pos;
      look_pos.z += orbit_z_nudge(p.orbit, p.player_is_morphed);
      // C++: `camEye = vec4(lookPos,1) - (angle * vec4{distance,0,0,1})`; the
      // quat rotates the xyz, the vec4 subtraction truncates to vec3.
      let eye = look_pos - (angle * Vec3::new(p.distance, 0.0, 0.0));
      manual_camera_pos = look_pos;
      (
        proj,
        glam::camera::rh::view::look_at_mat4(eye, look_pos, p.up),
      )
    }
    CameraMode::GameCam => (p.game_cam.perspective, p.game_cam.transform.inverse()),
    CameraMode::Detached => {
      let proj = perspective(p.fov, p.aspect, p.z_near, p.z_far);
      let angle = quat_from_euler(Vec3::new(0.0, p.pitch, p.yaw));
      let eye = p.manual_camera_pos - (angle * Vec3::new(p.distance, 0.0, 0.0));
      (
        proj,
        glam::camera::rh::view::look_at_mat4(eye, p.manual_camera_pos, p.up),
      )
    }
  };
  // Replaces the `glm::decompose(camView, …)` block (`WorldRenderer.cpp:297-310`)
  // — only `camEye` is consumed downstream this phase (the shader `viewPos`);
  // `camPointing` / `camViewport` are P8.4.5. The camera-to-world translation is
  // the true eye position.
  let eye = view.inverse().w_axis.truncate();
  CameraResult {
    projection,
    view,
    eye,
    manual_camera_pos,
  }
}

/// Pure `mesh_by_mrea` / GPU-cache bookkeeping for one area, factored out of
/// [`WorldRenderer::update_areas`] so it's testable without a GPU device
/// (`WorldRenderer.cpp:155-167`). `loaded` = `isPostConstructed`; `load`
/// produces the CPU mesh on a cache miss.
fn reconcile_area<F: FnOnce() -> Option<CollisionMesh>>(
  mesh_by_mrea: &mut HashMap<u32, CollisionMesh>,
  gpu_mesh_by_mrea: &mut HashMap<u32, DynamicMesh>,
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

fn read_vec3_member(ctx: &Ctx, parent: &GameInstance, name: &str) -> Option<Vec3> {
  read_as_vec3(ctx, &parent.get_member(ctx, name)?)
}

pub struct WorldRenderer {
  // --- camera params (`WorldRenderer.hpp:83-113` defaults) ---
  pub aspect: f32,
  pub fov: f32,
  pub z_near: f32,
  pub z_far: f32,
  pub yaw: f32,
  pub pitch: f32,
  pub distance: f32,
  pub up: Vec3,
  pub manual_camera_pos: Vec3,
  pub light_dir: Vec3,
  pub cam_line_length: f32,
  pub culling: CullType,
  pub camera_mode: CameraMode,
  pub orbit_player_camera_origin: OrbitPlayerCameraOrigin,

  // --- cached per-frame camera state ---
  pub cam_projection: Mat4,
  pub cam_view: Mat4,
  pub cam_eye: Vec3,
  pub game_cam: GameCamera,

  // --- cached per-frame player state ---
  pub player_pos: Vec3,
  // P8.4.3: consumed by drawPlayer / renderEntities
  pub player_velocity: Vec3,
  // P8.4.3: consumed by drawPlayer / renderEntities
  pub player_orientation: Quat,
  pub player_is_morphed: bool,
  pub last_known_non_colliding_pos: Vec3,
  // P8.4.3: consumed by drawPlayer / renderEntities
  pub player_look_vec: Vec3,

  // --- GPU state ---
  size: (u32, u32),
  color: wgpu::TextureView,
  depth: wgpu::TextureView,
  pipelines: WorldPipelines,
  render_buff: crate::gl::immediate::ImmediateModeBuffer,
  translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer,
  opaque_tris: DynamicMesh,
  opaque_lines: DynamicMesh,
  translucent_tris: DynamicMesh,
  translucent_lines: DynamicMesh,
  mesh_by_mrea: HashMap<u32, CollisionMesh>,
  gpu_mesh_by_mrea: HashMap<u32, DynamicMesh>,
}

impl WorldRenderer {
  pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
    let size = clamp_size(size);
    let (color, depth) = create_targets(device, size);
    let pipelines = WorldPipelines::new(device, WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT);
    Self {
      aspect: 0.0,
      fov: 45.0,
      z_near: 0.1,
      z_far: 10000.0,
      yaw: 0.0,
      pitch: 0.3,
      distance: 10.0,
      up: Vec3::new(0.0, 0.0, 1.0),
      manual_camera_pos: Vec3::ZERO,
      light_dir: Vec3::new(0.1, 0.2, 0.9),
      cam_line_length: 10.0,
      culling: CullType::Back,
      camera_mode: CameraMode::FollowPlayer,
      orbit_player_camera_origin: OrbitPlayerCameraOrigin::Center,
      cam_projection: Mat4::IDENTITY,
      cam_view: Mat4::IDENTITY,
      cam_eye: Vec3::ZERO,
      game_cam: GameCamera::default(),
      player_pos: Vec3::ZERO,
      player_velocity: Vec3::ZERO,
      player_orientation: Quat::IDENTITY,
      player_is_morphed: false,
      last_known_non_colliding_pos: Vec3::ZERO,
      player_look_vec: Vec3::ZERO,
      size,
      color,
      depth,
      pipelines,
      // ports `WorldRenderer::init` (`WorldRenderer.cpp:115-118`).
      render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      opaque_tris: DynamicMesh::new(device, "world-opaque-tris", Topology::Triangles),
      opaque_lines: DynamicMesh::new(device, "world-opaque-lines", Topology::Lines),
      translucent_tris: DynamicMesh::new(device, "world-translucent-tris", Topology::Triangles),
      translucent_lines: DynamicMesh::new(device, "world-translucent-lines", Topology::Lines),
      mesh_by_mrea: HashMap::new(),
      gpu_mesh_by_mrea: HashMap::new(),
    }
  }

  /// Recreate the colour + depth targets if `size` changed. Returns `true` when
  /// they were recreated (same contract as the deleted `SpikeScene::resize`).
  pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) -> bool {
    let size = clamp_size(size);
    if size == self.size {
      return false;
    }
    let (color, depth) = create_targets(device, size);
    self.color = color;
    self.depth = depth;
    self.size = size;
    true
  }

  /// The offscreen colour target — handed to egui as a user texture.
  pub fn color_view(&self) -> &wgpu::TextureView {
    &self.color
  }

  /// Ports `WorldRenderer::update` (`WorldRenderer.cpp:120-151`) + the
  /// camera-setup block (`258-310`) + the CPU-side ghost-cube / camera-line
  /// accumulation (`321-334`).
  pub fn update(&mut self, ctx: &Ctx, input: &WorldInput, viewport_size: (u32, u32)) {
    self.update_areas(ctx);

    self.pitch += input.cam_pitch;
    self.yaw += input.cam_yaw;
    self.distance += input.cam_zoom;
    let lim = std::f32::consts::FRAC_PI_2 - 0.1;
    self.pitch = self.pitch.clamp(-lim, lim);
    self.distance = self.distance.clamp(1.0, 100.0);

    // C++ sets `aspect` from the framebuffer elsewhere; derive it here.
    self.aspect = viewport_size.0 as f32 / viewport_size.1.max(1) as f32;

    // --- player reads (keep last good value on a `None`) ---
    let sm = get_state_manager();
    if let Some(player) = sm.get_member(ctx, "player") {
      if let Some(tf) = player
        .get_member(ctx, "transform")
        .and_then(|m| read_as_transform(ctx, &m))
      {
        self.player_pos = tf.w_axis.truncate();
      }
      if let Some(v) = read_vec3_member(ctx, &player, "velocity") {
        self.player_velocity = v;
      }
      if let Some(v) = player
        .get_member(ctx, "lastNonCollidingState")
        .and_then(|lncs| read_vec3_member(ctx, &lncs, "translation"))
      {
        self.last_known_non_colliding_pos = v;
      }
      if let Some(v) = read_vec3_member(ctx, &player, "lookDir") {
        self.player_look_vec = v;
      }
      if let Some(q) = player
        .get_member(ctx, "orientation")
        .and_then(|m| read_as_quat(ctx, &m))
      {
        self.player_orientation = q;
      }
      if let Some(morph) = player
        .get_member(ctx, "morphState")
        .and_then(|m| m.read_u32(ctx))
      {
        self.player_is_morphed = morph == 1; // EPlayerMorphBallState::Morphed
      }
    }

    // --- camera manager -> in-game camera (keep last good value on a `None`) ---
    if let Some(cam_mgr) = sm.get_member(ctx, "cameraManager")
      && let Some(cam_id) = cam_mgr
        .get_member(ctx, "curCameraId")
        .and_then(|m| m.read_u16(ctx))
      && let Some(mut camera) = get_object_by_entity_id(ctx, cam_id)
    {
      // C++ `camera.type = "CGameCamera"` — assume the active camera is one.
      camera.type_name = "CGameCamera".to_string();
      if let Some(m) = camera
        .get_member(ctx, "perspectiveMatrix")
        .and_then(|m| read_as_matrix4f(ctx, &m))
      {
        self.game_cam.perspective = m;
      }
      if let Some(m) = camera
        .get_member(ctx, "transform")
        .and_then(|m| read_as_transform(ctx, &m))
      {
        self.game_cam.transform = m;
      }
      if let Some(v) = camera.get_member(ctx, "fov").and_then(|m| m.read_f32(ctx)) {
        self.game_cam.fov = v;
      }
      if let Some(v) = camera
        .get_member(ctx, "znear")
        .and_then(|m| m.read_f32(ctx))
      {
        self.game_cam.znear = v;
      }
      if let Some(v) = camera.get_member(ctx, "zfar").and_then(|m| m.read_f32(ctx)) {
        self.game_cam.zfar = v;
      }
      if let Some(v) = camera
        .get_member(ctx, "aspect")
        .and_then(|m| m.read_f32(ctx))
      {
        self.game_cam.aspect = v;
      }
    }

    // --- camera setup ---
    let res = compute_camera(&CameraParams {
      camera_mode: self.camera_mode,
      orbit: self.orbit_player_camera_origin,
      fov: self.fov,
      aspect: self.aspect,
      z_near: self.z_near,
      z_far: self.z_far,
      pitch: self.pitch,
      yaw: self.yaw,
      distance: self.distance,
      up: self.up,
      player_is_morphed: self.player_is_morphed,
      last_known_non_colliding_pos: self.last_known_non_colliding_pos,
      manual_camera_pos: self.manual_camera_pos,
      game_cam: self.game_cam,
    });
    self.cam_projection = res.projection;
    self.cam_view = res.view;
    self.cam_eye = res.eye;
    self.manual_camera_pos = res.manual_camera_pos;

    // --- CPU geometry into the immediate buffers (`WorldRenderer.cpp:321-334`) ---
    self.render_buff.clear();
    self.translucent_render_buff.clear();

    self
      .translucent_render_buff
      .set_transform(Mat4::from_translation(self.last_known_non_colliding_pos));
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(
        Vec3::new(-0.5, -0.5, 0.0),
        Vec3::new(0.5, 0.5, 2.7),
        Vec4::new(1.0, 0.5, 0.5, 0.5),
      ));

    self.render_buff.set_transform(Mat4::IDENTITY);
    self
      .render_buff
      .add_lines(&shapes::generate_camera_line_segments(
        self.game_cam.perspective,
        self.game_cam.transform,
        self.cam_line_length,
      ));
  }

  /// Ports `WorldRenderer::updateAreas` (`WorldRenderer.cpp:153-168`).
  fn update_areas(&mut self, ctx: &Ctx) {
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

  /// Ports the GPU half of `WorldRenderer::render`
  /// (`WorldRenderer.cpp:336-406`, minus `renderEntities`). Adds one render pass
  /// into the offscreen `(color, depth)` target.
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
        let mut dm = DynamicMesh::new(device, "collision-mesh", Topology::Triangles);
        dm.upload(device, queue, &self.mesh_by_mrea[&mrea].verts);
        self.gpu_mesh_by_mrea.insert(mrea, dm);
      }
    }
    self
      .gpu_mesh_by_mrea
      .retain(|k, _| self.mesh_by_mrea.contains_key(k));

    // Per-mesh AABB wireframe boxes (`WorldRenderer.cpp:374-378`) — done here so
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

    // model is identity for every draw this phase: the immediate buffers bake
    // per-vertex transforms and collision verts are already world-space
    // (`WorldRenderer.cpp:339` / `386` / `397`).
    let uniforms = WorldUniforms::from_matrices(
      Mat4::IDENTITY,
      self.cam_view,
      self.cam_projection,
      self.cam_eye,
      self.light_dir.normalize(),
    );
    self.pipelines.set_uniforms(queue, &uniforms);

    let mesh_cull = match self.culling {
      CullType::Back => Some(wgpu::Face::Back),
      CullType::Front => Some(wgpu::Face::Front),
      CullType::None => None,
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

    // (a) collision meshes — honour `self.culling` (`WorldRenderer.cpp:357-372`).
    pass.set_pipeline(self.pipelines.mesh(false, mesh_cull));
    for dm in self.gpu_mesh_by_mrea.values() {
      dm.draw(&mut pass);
    }

    // (b) opaque immediate buffer — tris always back-culled
    // (`WorldRenderer.cpp:382-391`), lines never culled.
    pass.set_pipeline(self.pipelines.mesh(false, Some(wgpu::Face::Back)));
    self.opaque_tris.draw(&mut pass);
    pass.set_pipeline(&self.pipelines.line_opaque);
    self.opaque_lines.draw(&mut pass);

    // (c) translucent immediate buffer (`WorldRenderer.cpp:393-403`).
    pass.set_pipeline(self.pipelines.mesh(true, Some(wgpu::Face::Back)));
    self.translucent_tris.draw(&mut pass);
    pass.set_pipeline(&self.pipelines.line_translucent);
    self.translucent_lines.draw(&mut pass);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn base_params() -> CameraParams {
    CameraParams {
      camera_mode: CameraMode::FollowPlayer,
      orbit: OrbitPlayerCameraOrigin::Bottom,
      fov: 45.0,
      aspect: 1.5,
      z_near: 0.1,
      z_far: 10000.0,
      pitch: 0.0,
      yaw: 0.0,
      distance: 10.0,
      up: Vec3::new(0.0, 0.0, 1.0),
      player_is_morphed: false,
      last_known_non_colliding_pos: Vec3::ZERO,
      manual_camera_pos: Vec3::ZERO,
      game_cam: GameCamera::default(),
    }
  }

  fn approx(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-3, "{a:?} != {b:?}");
  }

  #[test]
  fn quat_from_euler_matches_glm_half_angle_formula() {
    for euler in [
      Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
      Vec3::new(0.3, 0.5, 0.7),
      Vec3::new(0.0, -1.2, 2.1),
    ] {
      let h = euler * 0.5;
      let (sx, cx) = h.x.sin_cos();
      let (sy, cy) = h.y.sin_cos();
      let (sz, cz) = h.z.sin_cos();
      let want = Quat::from_xyzw(
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
      );
      let got = quat_from_euler(euler);
      assert!((got.x - want.x).abs() < 1e-6);
      assert!((got.y - want.y).abs() < 1e-6);
      assert!((got.z - want.z).abs() < 1e-6);
      assert!((got.w - want.w).abs() < 1e-6);
    }
  }

  #[test]
  fn quat_from_euler_yaw_only_rotates_about_z() {
    // euler = (0, 0, yaw) -> pure Z rotation.
    let q = quat_from_euler(Vec3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
    approx(q * Vec3::X, Vec3::Y);
  }

  #[test]
  fn follow_player_places_eye_behind_look_pos() {
    let mut p = base_params();
    p.last_known_non_colliding_pos = Vec3::new(1.0, 2.0, 3.0);
    let r = compute_camera(&p);
    // yaw=pitch=0 -> angle = identity -> eye = lookPos - (distance,0,0).
    approx(r.eye, Vec3::new(1.0 - 10.0, 2.0, 3.0));
    approx(r.manual_camera_pos, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(
      r.projection,
      perspective(p.fov, p.aspect, p.z_near, p.z_far)
    );
  }

  #[test]
  fn follow_player_center_origin_nudges_look_pos_z() {
    let mut p = base_params();
    p.orbit = OrbitPlayerCameraOrigin::Center;
    p.player_is_morphed = false;
    let r = compute_camera(&p);
    // lookPos.z += 1.35; eye = lookPos - (10,0,0).
    approx(r.eye, Vec3::new(-10.0, 0.0, 1.35));
  }

  #[test]
  fn game_cam_uses_the_read_matrices() {
    let mut p = base_params();
    p.camera_mode = CameraMode::GameCam;
    p.game_cam.perspective = Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));
    p.game_cam.transform = Mat4::from_translation(Vec3::new(5.0, 6.0, 7.0));
    let r = compute_camera(&p);
    assert_eq!(r.projection, p.game_cam.perspective);
    assert_eq!(r.view, p.game_cam.transform.inverse());
    // eye = view.inverse().w_axis = transform translation.
    approx(r.eye, Vec3::new(5.0, 6.0, 7.0));
  }

  #[test]
  fn detached_orbits_manual_camera_pos_without_z_nudge() {
    let mut p = base_params();
    p.camera_mode = CameraMode::Detached;
    p.orbit = OrbitPlayerCameraOrigin::Center; // ignored in Detached
    p.manual_camera_pos = Vec3::new(4.0, 0.0, 0.0);
    p.distance = 3.0;
    let r = compute_camera(&p);
    approx(r.eye, Vec3::new(1.0, 0.0, 0.0));
  }

  #[test]
  fn reconcile_area_adds_then_evicts() {
    let mut cpu: HashMap<u32, CollisionMesh> = HashMap::new();
    let mut gpu: HashMap<u32, DynamicMesh> = HashMap::new();

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
    let mut cpu: HashMap<u32, CollisionMesh> = HashMap::new();
    let mut gpu: HashMap<u32, DynamicMesh> = HashMap::new();
    reconcile_area(&mut cpu, &mut gpu, 0x22, true, || None);
    assert!(cpu.is_empty());
  }
}
