//! `WorldRenderer` — the live 3D world view.
//!
//! Renders to an offscreen target (`render` hands back a colour `TextureView`
//! for egui to composite), driven by the game's memory — the three camera modes,
//! the `mesh_by_mrea` collision-mesh cache + GPU upload, and the area-AABB /
//! camera-frustum line overlays.
//!
//! Deviations from the C++ are called out at each site; the load-bearing ones:
//! - Camera reads keep the last good value on a `None` (mid-load) rather than
//!   zeroing — a zeroed transform would snap the camera to the origin every load.
//! - `fov` is passed to `perspective` unconverted, exactly as the C++ passes it
//!   to `glm::perspective` (see [`camera::compute_camera`]).
//! - `glm::decompose` -> `cam_eye = cam_view.inverse().w_axis` (only `cam_eye`
//!   is consumed).
//!
//! Split across submodules: [`types`] (plain data/config), [`camera`] (pure
//! camera math), [`entities`] (`renderEntities` + the per-class `draw*`
//! functions), [`gpu`] (offscreen targets, the collision-mesh cache, the render
//! pass), and [`ui`] (the egui status windows / menu bar / camera controls).

mod camera;
mod entities;
mod gpu;
mod shadow;
mod ui;

use std::collections::{BTreeMap, HashMap, HashSet};

use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::gl::mesh::DynamicMesh;
use crate::gl::shader::WorldPipelines;
use crate::gl::{WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT, shapes};
use crate::mem::game_object_utils::{TUniqueID, get_object_by_entity_id};
use crate::mem::globals::get_state_manager;
use crate::mem::math_utils::{read_as_matrix4f, read_as_quat, read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::CollisionMesh;

pub use camera::quat_from_euler;
pub use types::{
  ActorRenderConfig, CameraMode, CullType, GameCamera, OrbitPlayerCameraOrigin, PlayerClipConfig,
  PlayerGhost, ShadowConfig, TextOverlay, TriggerRenderConfig, WorldInput,
};

mod types;

fn read_vec3_member(ctx: &Ctx, parent: &GameInstance, name: &str) -> Option<Vec3> {
  read_as_vec3(ctx, &parent.get_member(ctx, name)?)
}

pub struct WorldRenderer {
  // --- camera params ---
  pub aspect: f32,
  pub fov: f32,
  pub z_near: f32,
  pub z_far: f32,
  pub yaw: f32,
  pub pitch: f32,
  pub distance: f32,
  pub up: Vec3,
  pub manual_camera_pos: Vec3,
  /// Scene light azimuth (radians, rotation around Z). See
  /// [`shadow::dir_from_azimuth_elevation`].
  pub light_azimuth: f32,
  /// Scene light elevation (radians from the horizon; `FRAC_PI_2` = straight
  /// up). See [`shadow::dir_from_azimuth_elevation`].
  pub light_elevation: f32,
  pub cam_line_length: f32,
  pub culling: CullType,
  pub camera_mode: CameraMode,
  pub orbit_player_camera_origin: OrbitPlayerCameraOrigin,
  pub trigger_render_config: TriggerRenderConfig,
  pub actor_render_config: ActorRenderConfig,
  pub player_clip_config: PlayerClipConfig,
  pub shadow_config: ShadowConfig,
  /// The detached-camera move-speed multiplier, driven by the "Speed" slider in
  /// the Camera menu.
  pub manual_camera_speed: f32,
  /// This is really app-shell state; it is parked on `WorldRenderer`
  /// for now so the menu bar and the Camera Controls window can share it
  /// without new app plumbing.
  pub show_exact_camera_controls: bool,

  // --- cached per-frame camera state ---
  pub cam_projection: Mat4,
  pub cam_view: Mat4,
  pub cam_eye: Vec3,
  /// Pixel-space viewport `[x, y, width, height]` for [`camera::project`].
  /// Aet in [`WorldRenderer::resize`] and again each [`WorldRenderer::update`].
  pub cam_viewport: [f32; 4],
  pub game_cam: GameCamera,

  /// Screen-space labels accumulated this frame (HP / item / fuse counts).
  /// Cleared at the top of every [`WorldRenderer::update`].
  pub text_overlays: Vec<TextOverlay>,

  // --- cached per-frame player state ---
  /// The live player, read from `g_stateManager["player"]` each frame. Its
  /// `position` / `orientation` / `velocity` / `is_morphed` feed `draw_player`
  /// and the camera; a `None` on any sub-read keeps the last good value.
  pub player: PlayerGhost,
  /// Saved player ghosts  (using hotkeys)
  pub player_ghosts: [PlayerGhost; 5],
  pub last_known_non_colliding_pos: Vec3,
  pub player_look_vec: Vec3,

  // --- GPU state ---
  size: (u32, u32),
  color: wgpu::TextureView,
  depth: wgpu::TextureView,
  pipelines: WorldPipelines,
  render_buff: crate::gl::immediate::ImmediateModeBuffer,
  translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer,
  /// Player / ghost model tris — kept separate so they can be drawn with the
  /// `bind_group_noclip` uniforms (the near-player bayer cutout must not eat the
  /// player model itself).
  player_render_buff: crate::gl::immediate::ImmediateModeBuffer,
  player_translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer,
  opaque_tris: DynamicMesh,
  opaque_lines: DynamicMesh,
  translucent_tris: DynamicMesh,
  translucent_lines: DynamicMesh,
  player_tris: DynamicMesh,
  player_translucent_tris: DynamicMesh,
  mesh_by_mrea: HashMap<u32, CollisionMesh>,
  gpu_mesh_by_mrea: HashMap<u32, DynamicMesh>,
}

impl WorldRenderer {
  pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
    let size = gpu::clamp_size(size);
    let (color, depth) = gpu::create_targets(device, size);
    let pipelines = WorldPipelines::new(device, WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT);
    Self {
      aspect: 0.0,
      fov: 45.0f32.to_radians(),
      z_near: 0.1,
      z_far: 10000.0,
      yaw: 0.0,
      pitch: 0.3,
      distance: 10.0,
      up: Vec3::new(0.0, 0.0, 1.0),
      manual_camera_pos: Vec3::ZERO,
      // Matches the pre-refactor fixed `light_dir` of `(0.1, 0.2, 0.9)`.
      light_azimuth: 1.1071487,
      light_elevation: 1.3272751,
      cam_line_length: 10.0,
      culling: CullType::Back,
      camera_mode: CameraMode::FollowPlayer,
      orbit_player_camera_origin: OrbitPlayerCameraOrigin::Center,
      trigger_render_config: TriggerRenderConfig::default(),
      actor_render_config: ActorRenderConfig::default(),
      player_clip_config: PlayerClipConfig::default(),
      shadow_config: ShadowConfig::default(),
      manual_camera_speed: 1.0,
      show_exact_camera_controls: false,
      cam_projection: Mat4::IDENTITY,
      cam_view: Mat4::IDENTITY,
      cam_eye: Vec3::ZERO,
      cam_viewport: [0.0, 0.0, size.0 as f32, size.1 as f32],
      game_cam: GameCamera::default(),
      text_overlays: Vec::new(),
      player: PlayerGhost::default(),
      player_ghosts: [PlayerGhost::default(); 5],
      last_known_non_colliding_pos: Vec3::ZERO,
      player_look_vec: Vec3::ZERO,
      size,
      color,
      depth,
      pipelines,
      // mirrors `WorldRenderer::init`.
      render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      player_render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      player_translucent_render_buff: crate::gl::immediate::ImmediateModeBuffer::new(),
      opaque_tris: DynamicMesh::new(device, "world-opaque-tris"),
      opaque_lines: DynamicMesh::new(device, "world-opaque-lines"),
      translucent_tris: DynamicMesh::new(device, "world-translucent-tris"),
      translucent_lines: DynamicMesh::new(device, "world-translucent-lines"),
      player_tris: DynamicMesh::new(device, "world-player-tris"),
      player_translucent_tris: DynamicMesh::new(device, "world-player-translucent-tris"),
      mesh_by_mrea: HashMap::new(),
      gpu_mesh_by_mrea: HashMap::new(),
    }
  }

  /// Recreate the colour + depth targets if `size` changed. Returns `true` when
  /// they were recreated.
  pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) -> bool {
    let size = gpu::clamp_size(size);
    if size == self.size {
      return false;
    }
    let (color, depth) = gpu::create_targets(device, size);
    self.color = color;
    self.depth = depth;
    self.size = size;
    self.cam_viewport = [0.0, 0.0, size.0 as f32, size.1 as f32];
    true
  }

  /// Drop every accumulated screen-space label (start of frame).
  pub fn clear_text_overlays(&mut self) {
    self.text_overlays.clear();
  }

  /// Queue a screen-space label at `screen_pos` (pixels, Y-down — already
  /// flipped by the `getScreenspacePosFor*` helpers).
  pub fn add_text_overlay(&mut self, screen_pos: Vec2, text: String) {
    self.text_overlays.push(TextOverlay { screen_pos, text });
  }

  /// The offscreen colour target — handed to egui as a user texture.
  pub fn color_view(&self) -> &wgpu::TextureView {
    &self.color
  }

  /// The Shift+`1..5` branch of `PrimeWatch::processInput`: snapshot the live
  /// player into ghost slot `i` and enable it. Out-of-range `i` is a no-op.
  pub fn record_player_ghost(&mut self, i: usize) {
    let player = self.player;
    if let Some(ghost) = self.player_ghosts.get_mut(i) {
      ghost.enabled = true;
      ghost.position = player.position;
      ghost.orientation = player.orientation;
      ghost.velocity = player.velocity;
      ghost.is_morphed = player.is_morphed;
    }
  }

  /// The Ctrl+`1..5` branch of `PrimeWatch::processInput`: disable ghost slot `i`.
  pub fn clear_player_ghost(&mut self, i: usize) {
    if let Some(ghost) = self.player_ghosts.get_mut(i) {
      ghost.enabled = false;
    }
  }

  /// The `CameraMode::DETATCHED` WASD/QE block of `PrimeWatch::processInput`.
  /// `forward` / `right` / `up` are the net key contributions (e.g.
  /// `forward = W - S`).
  pub fn move_detached_camera(&mut self, forward: f32, right: f32, up: f32) {
    let angle = quat_from_euler(Vec3::new(0.0, 0.0, self.yaw));
    let fwd = angle * Vec3::new(1.0, 0.0, 0.0);
    let rgt = angle * Vec3::new(0.0, 1.0, 0.0);
    let up_axis = Vec3::new(0.0, 0.0, 1.0);
    let speed = self.manual_camera_speed * 0.2;
    self.manual_camera_pos += fwd * (forward * speed);
    self.manual_camera_pos += rgt * (right * speed);
    self.manual_camera_pos += up_axis * (up * speed);
  }

  /// `WorldRenderer::update` + the camera-setup block + the CPU-side ghost-cube
  /// / camera-line accumulation + `drawPlayer` / `renderEntities` — those C++
  /// `render()` calls happen here at the end of `update` since this port keeps
  /// all CPU accumulation in `update`.
  pub fn update(
    &mut self,
    ctx: &Ctx,
    input: &WorldInput,
    viewport_size: (u32, u32),
    objects: &BTreeMap<TUniqueID, GameInstance>,
    highlighted: &HashSet<u16>,
  ) {
    self.clear_text_overlays();
    self.update_areas(ctx);

    self.pitch += input.cam_pitch;
    self.yaw += input.cam_yaw;
    self.distance += input.cam_zoom;
    let lim = std::f32::consts::FRAC_PI_2 - 0.1;
    self.pitch = self.pitch.clamp(-lim, lim);
    self.distance = self.distance.clamp(1.0, 100.0);

    self.aspect = viewport_size.0 as f32 / viewport_size.1.max(1) as f32;

    // --- player reads (keep last good value on a `None`) ---
    let sm = get_state_manager();
    if let Some(player) = sm.get_member(ctx, "player") {
      if let Some(tf) = player
        .get_member(ctx, "transform")
        .and_then(|m| read_as_transform(ctx, &m))
      {
        self.player.position = tf.w_axis.truncate();
      }
      if let Some(v) = read_vec3_member(ctx, &player, "velocity") {
        self.player.velocity = v;
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
        self.player.orientation = q;
      }
      if let Some(morph) = player
        .get_member(ctx, "morphState")
        .and_then(|m| m.read_u32(ctx))
      {
        self.player.is_morphed = morph == 1; // EPlayerMorphBallState::Morphed
      }
    }

    // --- camera manager -> in-game camera (keep last good value on a `None`) ---
    if let Some(cam_mgr) = sm.get_member(ctx, "cameraManager")
      && let Some(cam_id) = cam_mgr
        .get_member(ctx, "curCameraId")
        .and_then(|m| m.read_u16(ctx))
      && let Some(mut camera) = get_object_by_entity_id(ctx, cam_id)
    {
      // Assume the active camera is a CGameCamera.
      // There seems to be a bug here at least sometimes, causing morph camera data to not pull in properly
      // TODO: add a view to debug this?
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
    let res = camera::compute_camera(&camera::CameraParams {
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
      player_is_morphed: self.player.is_morphed,
      last_known_non_colliding_pos: self.last_known_non_colliding_pos,
      manual_camera_pos: self.manual_camera_pos,
      game_cam: self.game_cam,
    });
    self.cam_projection = res.projection;
    self.cam_view = res.view;
    self.cam_eye = res.eye;
    self.manual_camera_pos = res.manual_camera_pos;
    self.cam_viewport = [
      0.0,
      0.0,
      viewport_size.0 as f32,
      viewport_size.1.max(1) as f32,
    ];

    // --- CPU geometry into the immediate buffers ---
    self.render_buff.clear();
    self.translucent_render_buff.clear();
    self.player_render_buff.clear();
    self.player_translucent_render_buff.clear();

    // Player collision
    self
      .player_translucent_render_buff
      .set_transform(Mat4::from_translation(self.last_known_non_colliding_pos));
    self
      .player_translucent_render_buff
      .add_tris(&shapes::generate_cube(
        Vec3::new(-0.5, -0.5, 0.0),
        Vec3::new(0.5, 0.5, 2.7),
        Vec4::new(1.0, 0.5, 0.5, 0.5),
      ));

    // In GameCam mode the view *is* the game camera, so the frustum lines sit exactly on
    // the screen edge and flicker in and out. Skip them in that mode.
    if self.camera_mode != CameraMode::GameCam {
      self.render_buff.set_transform(Mat4::IDENTITY);
      self
        .render_buff
        .add_lines(&shapes::generate_camera_line_segments(
          self.game_cam.perspective,
          self.game_cam.transform,
          self.cam_line_length,
        ));
    }

    // --- entities + player ---
    self.render_entities(ctx, objects, highlighted);

    let player = self.player;
    self.draw_player(&player, Vec4::ONE);
    let ghosts = self.player_ghosts;
    for ghost in ghosts {
      if ghost.enabled {
        // teal
        self.draw_player(&ghost, Vec4::new(0.0, 1.0, 1.0, 0.5));
      }
    }
  }
}
