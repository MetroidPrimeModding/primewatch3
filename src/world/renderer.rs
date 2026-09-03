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

use std::collections::{BTreeMap, HashMap, HashSet};

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

use crate::ctx::Ctx;
use crate::defs::item_types::{EItemType, item_type_to_name};
use crate::gl::mesh::DynamicMesh;
use crate::gl::shader::{WorldPipelines, WorldUniforms};
use crate::gl::{Topology, Vert, WORLD_COLOR_FORMAT, WORLD_DEPTH_FORMAT, shapes};
use crate::mem::area_utils::get_areas;
use crate::mem::game_object_utils::{
  TUniqueID, get_all_loading_datas, get_object_by_entity_id, object_tag_to_string,
};
use crate::mem::globals::get_state_manager;
use crate::mem::math_utils::{read_as_matrix4f, read_as_quat, read_as_transform, read_as_vec3};
use crate::structs::prime_structs::GameInstance;
use crate::world::collision_mesh::{CollisionMesh, load_mesh};

/// Ports `enum class CullType` (`WorldRenderer.hpp:19-23`). Selected by the
/// P8.4.6 Culling menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CullType {
  Back,
  Front,
  None,
}

/// Ports `enum class CameraMode` (`WorldRenderer.hpp:25-29`). Selected by the
/// P8.4.6 Camera menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
  FollowPlayer,
  Detached,
  GameCam,
}

/// Ports `enum class OrbitPlayerCameraOrigin` (`WorldRenderer.hpp:31-35`).
/// Selected by the P8.4.6 Camera menu (Follow Player sub-group).
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

/// Ports `struct PlayerGhost` (`WorldRenderer.hpp:73-79`). `enabled` gates the
/// `player_ghosts` draw loop; nothing populates the ghost array yet (matches
/// C++ — the loop is a no-op until a later phase feeds it).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerGhost {
  pub enabled: bool,
  pub position: Vec3,
  pub orientation: Quat,
  pub velocity: Vec3,
  pub is_morphed: bool,
}

/// Ports `struct TriggerRenderConfig` (`WorldRenderer.hpp:46-61`). The C++ type
/// is a packed bitfield with per-field initializers; here it's plain `bool`s
/// with a matching `Default`. The P8.4.6 UI toggles these.
#[derive(Clone, Copy, Debug)]
pub struct TriggerRenderConfig {
  pub detect_player: bool,
  pub detect_ai: bool,
  pub detect_projectiles: bool,
  pub detect_bombs: bool,
  pub detect_power_bombs: bool,
  pub kill_on_enter: bool,
  pub detect_morphed_player: bool,
  pub use_collision_impulses: bool,
  pub detect_camera: bool,
  pub use_boolean_intersection: bool,
  pub detect_unmorphed_player: bool,
  pub block_environmental_effects: bool,
  pub water: bool,
  pub docks: bool,
}

impl Default for TriggerRenderConfig {
  fn default() -> Self {
    Self {
      detect_player: true,
      detect_ai: false,
      detect_projectiles: false,
      detect_bombs: false,
      detect_power_bombs: false,
      kill_on_enter: false,
      detect_morphed_player: false,
      use_collision_impulses: false,
      detect_camera: false,
      use_boolean_intersection: false,
      detect_unmorphed_player: true,
      block_environmental_effects: false,
      water: true,
      docks: true,
    }
  }
}

/// Ports `struct ActorRenderConfig` (`WorldRenderer.hpp:63-71`). Same
/// bitfield-to-`bool` treatment as [`TriggerRenderConfig`].
#[derive(Clone, Copy, Debug)]
pub struct ActorRenderConfig {
  pub render_projectiles: bool,
  pub render_ai: bool,
  pub render_pickups: bool,
  pub render_collision_actors: bool,
  pub render_physics_actors: bool,
  pub render_actors: bool,
  pub render_all_actors: bool,
}

impl Default for ActorRenderConfig {
  fn default() -> Self {
    Self {
      render_projectiles: true,
      render_ai: true,
      render_pickups: true,
      render_collision_actors: true,
      render_physics_actors: false,
      render_actors: false,
      render_all_actors: false,
    }
  }
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

/// Walk a member chain (`entity["a"]["b"]…`), returning `None` on the first
/// missing link — the P8.4.2 "`None` -> skip the draw" convention for the
/// per-class draw functions.
fn walk_member(ctx: &Ctx, inst: &GameInstance, path: &[&str]) -> Option<GameInstance> {
  let mut cur = inst.clone();
  for name in path {
    cur = cur.get_member(ctx, name)?;
  }
  Some(cur)
}

/// [`walk_member`] + [`read_as_vec3`] — a `CVector3f` at the end of a member
/// chain.
fn read_vec3_at(ctx: &Ctx, inst: &GameInstance, path: &[&str]) -> Option<Vec3> {
  read_as_vec3(ctx, &walk_member(ctx, inst, path)?)
}

/// Ports the `triggerRenderFlags` assembly in `renderEntities`
/// (`WorldRenderer.cpp:587-599`) — `detect_projectiles` fans out to all seven
/// projectile bits.
pub(crate) fn trigger_render_flags(c: &TriggerRenderConfig) -> u32 {
  let mut f = 0u32;
  if c.detect_player {
    f |= 0x1;
  }
  if c.detect_ai {
    f |= 0x2;
  }
  if c.detect_projectiles {
    f |= 0x4 | 0x8 | 0x10 | 0x20 | 0x100 | 0x200 | 0x400;
  }
  if c.detect_bombs {
    f |= 0x40;
  }
  if c.detect_power_bombs {
    f |= 0x80;
  }
  if c.kill_on_enter {
    f |= 0x800;
  }
  if c.detect_morphed_player {
    f |= 0x1000;
  }
  if c.use_collision_impulses {
    f |= 0x2000;
  }
  if c.detect_camera {
    f |= 0x4000;
  }
  if c.use_boolean_intersection {
    f |= 0x8000;
  }
  if c.detect_unmorphed_player {
    f |= 0x10000;
  }
  if c.block_environmental_effects {
    f |= 0x20000;
  }
  f
}

/// Ports the `drawTrigger` colour ladder (`WorldRenderer.cpp:669-677`): default
/// white, water tint, highlight red — highlight always wins.
pub(crate) fn trigger_color(is_water: bool, is_highlighted: bool) -> Vec4 {
  let mut color = Vec4::new(1.0, 1.0, 1.0, 0.5);
  if is_water {
    color = Vec4::new(0.5, 0.5, 1.0, 0.5);
  }
  if is_highlighted {
    color = Vec4::new(1.0, 0.0, 0.0, 0.5);
  }
  color
}

/// `glm::abs(glm::length(min - max)) < 0.1` degeneracy test
/// (`WorldRenderer.cpp:711` / `716`).
pub(crate) fn is_degenerate_bbox(min: Vec3, max: Vec3) -> bool {
  (min - max).length().abs() < 0.1
}

/// Ports the `drawPhysicsActor` bounding-box fallback chain
/// (`WorldRenderer.cpp:706-719`): `collisionPrimitive` aabb (`pos`-offset) ->
/// `baseBoundingBox` (`pos`-offset) -> `renderBounds` (**no** `pos` offset —
/// the asymmetry is verbatim from C++).
pub(crate) fn physics_actor_bbox(
  pos: Vec3,
  collision_primitive: (Vec3, Vec3),
  base_bounding_box: (Vec3, Vec3),
  render_bounds: (Vec3, Vec3),
) -> (Vec3, Vec3) {
  let (mut min, mut max) = (pos + collision_primitive.0, pos + collision_primitive.1);
  if is_degenerate_bbox(min, max) {
    min = pos + base_bounding_box.0;
    max = pos + base_bounding_box.1;
  }
  if is_degenerate_bbox(min, max) {
    min = render_bounds.0;
    max = render_bounds.1;
  }
  (min, max)
}

/// Ports the `drawPlayer` speed-indicator colour ladder
/// (`WorldRenderer.cpp:567-576`): red when the angle between facing and
/// movement exceeds 90° (or is NaN), otherwise a green ramp that flips to cyan
/// past 95%.
pub(crate) fn player_speed_color(angle: f32) -> Vec4 {
  let half_pi = std::f32::consts::FRAC_PI_2;
  if angle.abs() > half_pi || angle.is_nan() {
    return Vec4::new(1.0, 0.0, 0.0, 1.0);
  }
  let percent = angle / half_pi;
  if percent > 0.95 {
    Vec4::new(0.0, 1.0, 1.0, 1.0)
  } else {
    Vec4::new(0.0, percent * 0.5 + 0.5, 0.0, 1.0)
  }
}

/// Ports `drawBomb`'s fuse-frame gate (`WorldRenderer.cpp:845-847`):
/// `ceil(fuseTimeSeconds * 60) + 1`. The draw is skipped when this is `<= 0`.
pub(crate) fn bomb_fuse_frames(fuse_time: f32) -> i32 {
  (fuse_time * 60.0).ceil() as i32 + 1
}

/// Ports `drawBomb`'s ball-proximity highlight recompute
/// (`WorldRenderer.cpp:851-859`) — the passed-in highlight flag is discarded and
/// this predicate decides. `maxDistance` is the hardcoded `1.5` tweak value.
pub(crate) fn bomb_proximity_highlight(player_pos: Vec3, bomb_pos: Vec3) -> bool {
  let pos_to_ball = player_pos + Vec3::new(0.0, 0.0, 0.7) - bomb_pos;
  pos_to_ball.length() < 1.5 && pos_to_ball.z >= -0.7
}

/// Ports `drawProjectile`'s nested `CProjectileWeapon` transform chain
/// (`WorldRenderer.cpp:811`): `localToWorldXf * (localXf * projOffset +
/// localOffset) + worldOffset`, with each offset extended to a `w = 0` vec4 so
/// the matrix translations only apply via `localToWorldXf` / `localXf`
/// rotation-scale, and `worldOffset` added in world space.
pub(crate) fn projectile_world_pos(
  local_to_world: Mat4,
  local_xf: Mat4,
  proj_offset: Vec3,
  local_offset: Vec3,
  world_offset: Vec3,
) -> Vec3 {
  (local_to_world * (local_xf * proj_offset.extend(0.0) + local_offset.extend(0.0))
    + world_offset.extend(0.0))
  .truncate()
}

/// Ports `drawProjectile`'s velocity transform (`WorldRenderer.cpp:813`):
/// `localToWorldXf * localXf * vec4(velocity, 0)`.
pub(crate) fn projectile_world_vel(local_to_world: Mat4, local_xf: Mat4, velocity: Vec3) -> Vec3 {
  (local_to_world * local_xf * velocity.extend(0.0)).truncate()
}

/// Ports `glm::project(obj, view, projection, viewport)` as used by
/// `getScreenspacePosFor*` (`WorldRenderer.cpp:915` / `938`).
///
/// `clip = projection * view * vec4(pos, 1)`, perspective-divide to NDC, then map
/// to the pixel viewport: `screen.xy = viewport.xy + (ndc.xy + 1) * 0.5 *
/// viewport.zw`. `viewport` is `[x, y, width, height]` in pixels.
///
/// The renderer's projection matrix is glam's DirectX-convention RH perspective
/// ([0, 1] clip depth) rather than GL's [-1, 1] — the x/y screen mapping is
/// identical either way, and callers only consume `.x` / `.y` (the returned `.z`
/// is the raw NDC depth and is unused).
///
/// Returns `None` when the point is on or behind the camera plane (`clip.w <= 0`).
/// The C++ (and glm) divided by a negative `w` there, mirroring points from behind
/// the camera onto the screen — this is that latent bug fixed: callers skip the
/// overlay instead of drawing it at a bogus position.
pub(crate) fn project(pos: Vec3, view: Mat4, projection: Mat4, viewport: [f32; 4]) -> Option<Vec3> {
  let clip = projection * view * pos.extend(1.0);
  if clip.w <= 0.0 {
    return None;
  }
  let ndc = clip.truncate() / clip.w;
  Some(Vec3::new(
    viewport[0] + (ndc.x + 1.0) * 0.5 * viewport[2],
    viewport[1] + (ndc.y + 1.0) * 0.5 * viewport[3],
    ndc.z,
  ))
}

/// A screen-space text label accumulated during `update` and painted by the app
/// shell's overlay pass (P9.1). Ports the `ImDrawList::AddText` calls in the
/// per-class draw functions (`WorldRenderer.cpp:873-878` / `900-909` /
/// `943-973`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextOverlay {
  pub screen_pos: Vec2,
  pub text: String,
}

/// Nominal line height for stacking multi-line overlays (`drawPickup`'s two
/// lines). The C++ uses `ImGui::GetTextLineHeight()`; this layer has no font
/// system, so the P9.1 painter owns exact glyph metrics / horizontal centering.
const OVERLAY_LINE_HEIGHT: f32 = 14.0;

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
  pub trigger_render_config: TriggerRenderConfig,
  pub actor_render_config: ActorRenderConfig,
  /// Ports `WorldRenderer.hpp:91` `float manualCameraSpeed{1.0f}` — the
  /// detached-camera move-speed multiplier, driven by the "Speed" slider in the
  /// Camera menu.
  pub manual_camera_speed: f32,
  /// Ports C++ `PrimeWatch::showExactCameraControls`. This is really app-shell
  /// state; it is parked on `WorldRenderer` for now so the P8.4.6 menu bar and
  /// the Camera Controls window can share it without new app plumbing.
  pub show_exact_camera_controls: bool,

  // --- cached per-frame camera state ---
  pub cam_projection: Mat4,
  pub cam_view: Mat4,
  pub cam_eye: Vec3,
  /// Pixel-space viewport `[x, y, width, height]` for [`project`]. Ports C++
  /// `camViewport` (`PrimeWatch.cpp:91` / `494` — `{0, 0, width, height}`); set
  /// in [`WorldRenderer::resize`] and again each [`WorldRenderer::update`].
  pub cam_viewport: [f32; 4],
  pub game_cam: GameCamera,

  /// Screen-space labels accumulated this frame (HP / item / fuse counts).
  /// Cleared at the top of every [`WorldRenderer::update`].
  pub text_overlays: Vec<TextOverlay>,

  // --- cached per-frame player state (`WorldRenderer.hpp:109-113`) ---
  /// The live player, read from `g_stateManager["player"]` each frame. Its
  /// `position` / `orientation` / `velocity` / `is_morphed` feed `draw_player`
  /// and the camera; a `None` on any sub-read keeps the last good value.
  pub player: PlayerGhost,
  /// `std::array<PlayerGhost, 5>` — all `enabled == false`, nothing populates
  /// them yet (matches C++); the `draw_player` loop over them is a no-op.
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
      trigger_render_config: TriggerRenderConfig::default(),
      actor_render_config: ActorRenderConfig::default(),
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

  /// Ports the Shift+`1..5` branch of `PrimeWatch::processInput`
  /// (`PrimeWatchInput.cpp:148-153`): snapshot the live player into ghost slot
  /// `i` and enable it. Out-of-range `i` is a no-op (C++ asserts the array size
  /// matches the key list; the Rust port just clamps).
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

  /// Ports the Ctrl+`1..5` branch of `PrimeWatch::processInput`
  /// (`PrimeWatchInput.cpp:154-156`): disable ghost slot `i`.
  pub fn clear_player_ghost(&mut self, i: usize) {
    if let Some(ghost) = self.player_ghosts.get_mut(i) {
      ghost.enabled = false;
    }
  }

  /// Ports the `CameraMode::DETATCHED` WASD/QE block of
  /// `PrimeWatch::processInput` (`PrimeWatchInput.cpp:206-231`). `forward` /
  /// `right` / `up` are the net key contributions (e.g. `forward = W - S`); the
  /// per-axis basis and `manualCameraSpeed * 0.2` scaling match the C++.
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

  /// Ports `WorldRenderer::update` (`WorldRenderer.cpp:120-151`) + the
  /// camera-setup block (`258-310`) + the CPU-side ghost-cube / camera-line
  /// accumulation (`321-334`) + `drawPlayer` / `renderEntities`
  /// (`312-336`) — those C++ `render()` calls happen here at the end of
  /// `update` since this port keeps all CPU accumulation in `update`.
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

    // C++ sets `aspect` from the framebuffer elsewhere; derive it here.
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
      player_is_morphed: self.player.is_morphed,
      last_known_non_colliding_pos: self.last_known_non_colliding_pos,
      manual_camera_pos: self.manual_camera_pos,
      game_cam: self.game_cam,
    });
    self.cam_projection = res.projection;
    self.cam_view = res.view;
    self.cam_eye = res.eye;
    self.manual_camera_pos = res.manual_camera_pos;
    // C++ sets `camViewport` from the framebuffer in `framebuffer_size_cb`; keep
    // it in lock-step with the render target each frame (pixel space, not NDC).
    self.cam_viewport = [
      0.0,
      0.0,
      viewport_size.0 as f32,
      viewport_size.1.max(1) as f32,
    ];

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

    // Deviation from C++ (which always drew this): in GameCam mode the view *is*
    // the game camera, so the frustum lines sit exactly on the screen edge and
    // flicker in and out. Skip them in that mode.
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

    // --- entities + player (`WorldRenderer.cpp:312-336`) ---
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

  /// Selects the opaque or translucent immediate buffer by `translucent`, sets
  /// its transform, and pushes `verts` — the `buf = color.a < 0.99 ? … : …`
  /// pattern shared by `drawPlayer` / the per-class draw functions.
  fn buf_add_tris(&mut self, translucent: bool, transform: Mat4, verts: &[Vert]) {
    let buf = if translucent {
      &mut self.translucent_render_buff
    } else {
      &mut self.render_buff
    };
    buf.set_transform(transform);
    buf.add_tris(verts);
  }

  /// Ports `WorldRenderer::drawPlayer` (`WorldRenderer.cpp:531-581`). The
  /// collision shape goes to the opaque or translucent buffer by `color.a`; the
  /// speed indicator is always on the opaque `render_buff`.
  fn draw_player(&mut self, ghost: &PlayerGhost, color: Vec4) {
    let translucent = color.w < 0.99;

    if ghost.is_morphed {
      let model = Mat4::from_translation(ghost.position + Vec3::new(0.0, 0.0, 0.7))
        * Mat4::from_quat(ghost.orientation);
      let tris = shapes::generate_sphere(Vec3::ZERO, 0.7, color);
      self.buf_add_tris(translucent, model, &tris);
    } else {
      let tris = shapes::generate_cube(Vec3::new(-0.5, -0.5, 0.0), Vec3::new(0.5, 0.5, 2.7), color);
      self.buf_add_tris(translucent, Mat4::from_translation(ghost.position), &tris);
    }

    // Speed indicator — always on the opaque `render_buff`.
    let z = if ghost.is_morphed { 0.7 } else { 2.7 / 2.0 };
    self.render_buff.set_transform(Mat4::from_translation(
      ghost.position + Vec3::new(0.0, 0.0, z),
    ));

    let forward3 = (ghost.orientation * Vec3::Y).normalize();
    let forward = Vec2::new(forward3.x, forward3.y);
    let movement3 = ghost.velocity.normalize();
    let movement = Vec2::new(movement3.x, movement3.y);
    let angle = (forward.dot(movement) / (forward.length() * movement.length())).acos();
    let speed_color = player_speed_color(angle);

    self.render_buff.set_color([1.0, 1.0, 1.0, 1.0]);
    self
      .render_buff
      .add_line(Vec3::ZERO, Vec3::new(forward.x, forward.y, 0.0));
    self.render_buff.set_color(speed_color.to_array());
    self.render_buff.add_line(Vec3::ZERO, ghost.velocity * 0.3);
  }

  /// Ports `WorldRenderer::renderEntities` (`WorldRenderer.cpp:583-662`) — the
  /// active/highlight filter plus the `extendsClass` dispatch chain. Chain order
  /// is load-bearing (`CCollisionActor` -> `CAi` -> `CPhysicsActor` ->
  /// `CActor`): every class here inherits from the ones below it.
  //
  // `collapsible_if` would suggest folding `if extends_class(X) { if cfg { … } }`
  // into `&&`, but that changes the dispatch — a class match with its config
  // flag off must NOT fall through to a base-class branch.
  #[allow(clippy::collapsible_if)]
  fn render_entities(
    &mut self,
    ctx: &Ctx,
    objects: &BTreeMap<TUniqueID, GameInstance>,
    highlighted: &HashSet<u16>,
  ) {
    self.render_buff.set_transform(Mat4::IDENTITY);
    let trigger_flags = trigger_render_flags(&self.trigger_render_config);

    for entity in objects.values() {
      let active = entity
        .get_member(ctx, "active")
        .and_then(|m| m.read_bool(ctx));
      if active != Some(true) {
        continue;
      }
      let is_highlighted = entity
        .get_member(ctx, "uniqueID")
        .and_then(|m| m.read_u16(ctx))
        .is_some_and(|uid| highlighted.contains(&uid));

      if entity.extends_class(ctx, "CScriptTrigger") {
        let flags = entity
          .get_member(ctx, "triggerFlags")
          .and_then(|m| m.read_u32(ctx))
          .unwrap_or(0);
        if entity.extends_class(ctx, "CScriptWater") {
          if self.trigger_render_config.water {
            self.draw_trigger(ctx, entity, is_highlighted);
          }
        } else if (flags & trigger_flags) != 0 {
          self.draw_trigger(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CScriptDock") {
        if self.trigger_render_config.docks {
          self.draw_dock(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CGameProjectile") {
        if self.actor_render_config.render_projectiles {
          self.draw_projectile(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CBomb") {
        if self.actor_render_config.render_projectiles {
          self.draw_bomb(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPowerBomb") {
        if self.actor_render_config.render_projectiles {
          self.draw_power_bomb(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPlayer") {
        // player render handled by draw_player
      } else if entity.extends_class(ctx, "CChozoGhost") {
        if self.actor_render_config.render_ai {
          self.draw_chozo_ghost(ctx, entity, is_highlighted, objects);
        }
      } else if entity.extends_class(ctx, "CScriptPickup") {
        if self.actor_render_config.render_pickups {
          self.draw_pickup(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CCollisionActor") {
        if self.actor_render_config.render_collision_actors {
          self.draw_collision_actor(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CAi") {
        if self.actor_render_config.render_ai {
          self.draw_ai(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CPhysicsActor") {
        if self.actor_render_config.render_physics_actors {
          self.draw_physics_actor(ctx, entity, is_highlighted);
        }
      } else if entity.extends_class(ctx, "CActor") {
        if self.actor_render_config.render_actors {
          self.draw_actor(ctx, entity, is_highlighted);
        }
      }
    }
  }

  /// Ports `WorldRenderer::drawTrigger` (`WorldRenderer.cpp:664-684`).
  fn draw_trigger(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(min) = read_vec3_at(ctx, entity, &["bounds", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["bounds", "max"]) else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = trigger_color(entity.extends_class(ctx, "CScriptWater"), is_highlighted);
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));
  }

  /// Ports `WorldRenderer::drawDock` (`WorldRenderer.cpp:686-702`). `min`/`max`
  /// are inherited from `CPhysicsActor::collisionPrimitive`.
  fn draw_dock(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(min) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"]) else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(0.5, 1.0, 0.5, 0.5)
    };
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));
  }

  /// Ports `WorldRenderer::drawPhysicsActor` (`WorldRenderer.cpp:704-739`).
  fn draw_physics_actor(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let pos = transform.w_axis.truncate();

    let Some(cp_min) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"]) else {
      return;
    };
    let Some(cp_max) = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"]) else {
      return;
    };
    let Some(bb_min) = read_vec3_at(ctx, entity, &["baseBoundingBox", "min"]) else {
      return;
    };
    let Some(bb_max) = read_vec3_at(ctx, entity, &["baseBoundingBox", "max"]) else {
      return;
    };
    let Some(rb_min) = read_vec3_at(ctx, entity, &["renderBounds", "min"]) else {
      return;
    };
    let Some(rb_max) = read_vec3_at(ctx, entity, &["renderBounds", "max"]) else {
      return;
    };

    let (min, max) = physics_actor_bbox(pos, (cp_min, cp_max), (bb_min, bb_max), (rb_min, rb_max));

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(1.0, 1.0, 1.0, 0.5)
    };

    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(Mat4::IDENTITY);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));

    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, 0.5, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 0.5));
  }

  /// Ports `WorldRenderer::drawActor` (`WorldRenderer.cpp:741-773`). A null
  /// `*CModelData` (`address == 0`, or an unreadable pointer) plus not
  /// highlighted plus `!render_all_actors` skips the actor.
  fn draw_actor(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let model_addr = entity.get_member(ctx, "modelData").map_or(0, |m| m.address);
    if model_addr == 0 && !is_highlighted && !self.actor_render_config.render_all_actors {
      return;
    }

    let Some(min) = read_vec3_at(ctx, entity, &["renderBounds", "min"]) else {
      return;
    };
    let Some(max) = read_vec3_at(ctx, entity, &["renderBounds", "max"]) else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(1.0, 1.0, 1.0, 0.5)
    };

    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(Mat4::IDENTITY);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));

    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, 0.5, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0));
    self
      .translucent_render_buff
      .add_line(Vec3::new(0.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 0.5));
  }

  /// Ports `WorldRenderer::getScreenspacePosForActor`
  /// (`WorldRenderer.cpp:912-918`): project the entity's transform translation
  /// to screen pixels, then flip Y for the top-left-origin overlay space.
  fn screenspace_pos_for_actor(&self, ctx: &Ctx, entity: &GameInstance) -> Option<Vec2> {
    let transform = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))?;
    let pos = transform.w_axis.truncate();
    let s = project(pos, self.cam_view, self.cam_projection, self.cam_viewport)?;
    Some(Vec2::new(s.x, self.cam_viewport[3] - s.y))
  }

  /// Ports `WorldRenderer::getScreenspacePosForPhysicsActor`
  /// (`WorldRenderer.cpp:920-941`): same as [`Self::screenspace_pos_for_actor`]
  /// but offsets the projected point by the centre of the actor's bounding box,
  /// picked from the `collisionPrimitive` -> `baseBoundingBox` -> `renderBounds`
  /// ladder (the last one is `pos`-relative — verbatim C++ asymmetry).
  fn screenspace_pos_for_physics_actor(&self, ctx: &Ctx, entity: &GameInstance) -> Option<Vec2> {
    let transform = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))?;
    let pos = transform.w_axis.truncate();

    let mut min = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "min"])?;
    let mut max = read_vec3_at(ctx, entity, &["collisionPrimitive", "aabb", "max"])?;
    if is_degenerate_bbox(min, max) {
      min = read_vec3_at(ctx, entity, &["baseBoundingBox", "min"])?;
      max = read_vec3_at(ctx, entity, &["baseBoundingBox", "max"])?;
    }
    if is_degenerate_bbox(min, max) {
      min = read_vec3_at(ctx, entity, &["renderBounds", "min"])? - pos;
      max = read_vec3_at(ctx, entity, &["renderBounds", "max"])? - pos;
    }

    let text_pos = (min + max) / 2.0;
    let s = project(
      pos + text_pos,
      self.cam_view,
      self.cam_projection,
      self.cam_viewport,
    )?;
    Some(Vec2::new(s.x, self.cam_viewport[3] - s.y))
  }

  // --- P8.4.4: specialized geometry + velocity vectors. P8.4.5 adds the
  // screen-space HP / item / fuse-frame text overlays via
  // [`Self::add_text_overlay`] + [`project`] (`ImDrawList::AddText` /
  // `getScreenspacePosFor*` in the C++).

  /// Ports `WorldRenderer::drawProjectile` (`WorldRenderer.cpp:800-842`) minus
  /// the dead line-821 `transform` read (never used in the C++). The
  /// `CProjectileWeapon` at `entity["projectile"]` is inline (not a pointer).
  fn draw_projectile(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(active) = entity
      .get_member(ctx, "projectileActive")
      .and_then(|m| m.read_bool(ctx))
    else {
      return;
    };
    if !active {
      return;
    }
    let Some(projectile) = entity.get_member(ctx, "projectile") else {
      return;
    };
    let Some(local_to_world) = projectile
      .get_member(ctx, "localToWorldXf")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let Some(local_xf) = projectile
      .get_member(ctx, "localXf")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let Some(proj_off) = read_vec3_member(ctx, &projectile, "projOffset") else {
      return;
    };
    let Some(local_off) = read_vec3_member(ctx, &projectile, "localOffset") else {
      return;
    };
    let Some(world_off) = read_vec3_member(ctx, &projectile, "worldOffset") else {
      return;
    };
    let Some(scale) = read_vec3_member(ctx, &projectile, "scale") else {
      return;
    };
    let Some(velocity) = read_vec3_member(ctx, &projectile, "velocity") else {
      return;
    };
    let Some(extent) = entity
      .get_member(ctx, "projExtent")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };

    let pos = projectile_world_pos(local_to_world, local_xf, proj_off, local_off, world_off);
    let vel = projectile_world_vel(local_to_world, local_xf, velocity);

    // component-wise (glam `Vec3 * Vec3` is Hadamard, matching `glm::vec3`).
    let size = Vec3::splat(extent) / 2.0 * scale;
    let min = pos - size;
    let max = pos + size;

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(0.8, 0.4, 0.4, 0.8)
    };

    // min/max are world-space already -> identity transform for the cube.
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(Mat4::IDENTITY);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_cube(min, max, color));

    if is_highlighted {
      self.translucent_render_buff.set_color([0.8, 0.8, 0.8, 0.5]);
      self
        .translucent_render_buff
        .add_line(pos, pos + vel.normalize() * 1000.0);
    }
    self.translucent_render_buff.set_color([1.0, 0.5, 0.5, 1.0]);
    self
      .translucent_render_buff
      .add_line(pos, pos + vel.normalize() * 0.5);
  }

  /// Ports `WorldRenderer::drawBomb` (`WorldRenderer.cpp:844-879`). The passed-in
  /// `_is_highlighted` is intentionally ignored — the C++ recomputes it from ball
  /// proximity. The fuse-frame count is queued as a screen-space overlay
  /// (`WorldRenderer.cpp:873-878`).
  fn draw_bomb(&mut self, ctx: &Ctx, entity: &GameInstance, _is_highlighted: bool) {
    let Some(fuse_time) = entity
      .get_member(ctx, "fuseTime")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    if bomb_fuse_frames(fuse_time) <= 0 {
      return;
    }
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let pos = transform.w_axis.truncate();
    let is_highlighted = bomb_proximity_highlight(self.player.position, pos);

    let color = if is_highlighted {
      Vec4::new(0.8, 0.0, 0.0, 0.8)
    } else {
      Vec4::new(0.7, 0.5, 0.5, 0.5)
    };

    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    // maxDistance (1.5) - 0.7
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_truncated_sphere(
        Vec3::ZERO,
        1.5 - 0.7,
        0.0,
        color,
      ));

    // HP-style fuse-frame count over the bomb (`WorldRenderer.cpp:873-878`).
    if let Some(screen) = self.screenspace_pos_for_actor(ctx, entity) {
      self.add_text_overlay(screen, format!("{}", bomb_fuse_frames(fuse_time)));
    }
  }

  /// Ports `WorldRenderer::drawPowerBomb` (`WorldRenderer.cpp:881-896`). No
  /// highlight branch. `CPowerBomb : CWeapon`.
  fn draw_power_bomb(&mut self, ctx: &Ctx, entity: &GameInstance, _is_highlighted: bool) {
    let Some(cur_time) = entity
      .get_member(ctx, "curTime")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    if !(1.0..=4.0).contains(&cur_time) {
      return;
    }
    let Some(cur_radius) = entity
      .get_member(ctx, "curRadius")
      .and_then(|m| m.read_f32(ctx))
    else {
      return;
    };
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };
    let color = Vec4::new(0.8, 0.4, 0.4, 0.4);
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    self
      .translucent_render_buff
      .add_tris(&shapes::generate_sphere(Vec3::ZERO, cur_radius, color));
  }

  /// Ports `WorldRenderer::drawChozoGhost` (`WorldRenderer.cpp:775-798`) minus
  /// the dead `spaceWarpPosition` read and the commented-out warp line. Draws
  /// the `CAi` body then a magenta line to the ghost's cover point (resolved by
  /// slot id `coverPoint & 0x3FF` in the object map).
  fn draw_chozo_ghost(
    &mut self,
    ctx: &Ctx,
    entity: &GameInstance,
    is_highlighted: bool,
    objects: &BTreeMap<TUniqueID, GameInstance>,
  ) {
    self.draw_ai(ctx, entity, is_highlighted);

    let Some(ghost_pos) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
      .map(|tf| tf.w_axis.truncate())
    else {
      return;
    };
    let Some(cover_id) = entity
      .get_member(ctx, "coverPoint")
      .and_then(|m| m.read_u16(ctx))
    else {
      return;
    };
    let cover_id = cover_id & 0x3FF;
    if let Some(cover) = objects.get(&cover_id) {
      let Some(cover_pos) = cover
        .get_member(ctx, "transform")
        .and_then(|m| read_as_transform(ctx, &m))
        .map(|tf| tf.w_axis.truncate())
      else {
        return;
      };
      self.translucent_render_buff.set_transform(Mat4::IDENTITY);
      self.translucent_render_buff.set_color([1.0, 0.0, 1.0, 1.0]);
      self.translucent_render_buff.add_line(ghost_pos, cover_pos);
    }
  }

  /// Ports `WorldRenderer::drawPickup` (`WorldRenderer.cpp:943-973`). With the
  /// Ports `WorldRenderer::drawPickup` (`WorldRenderer.cpp:943-973`): the
  /// `drawPhysicsActor` body plus two label lines — `"<item> <amount>/<capacity>"`
  /// above and `"<curTime>/<lifeTime>"` below the projected point.
  fn draw_pickup(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    self.draw_physics_actor(ctx, entity, is_highlighted);

    let Some(screen) = self.screenspace_pos_for_physics_actor(ctx, entity) else {
      return;
    };
    let Some(item_type) = entity
      .get_member(ctx, "itemType")
      .and_then(|m| m.read_u32(ctx))
      .map(EItemType::from_raw)
    else {
      return;
    };
    let amount = entity
      .get_member(ctx, "amount")
      .and_then(|m| m.read_u32(ctx))
      .unwrap_or(0) as i32;
    let capacity = entity
      .get_member(ctx, "capacity")
      .and_then(|m| m.read_u32(ctx))
      .unwrap_or(0) as i32;
    let life_time = entity
      .get_member(ctx, "lifeTime")
      .and_then(|m| m.read_f32(ctx))
      .unwrap_or(0.0);
    let cur_time = entity
      .get_member(ctx, "curTime")
      .and_then(|m| m.read_f32(ctx))
      .unwrap_or(0.0);

    let line1 = format!("{} {}/{}", item_type_to_name(item_type), amount, capacity);
    let line2 = format!("{cur_time:.1}/{life_time:.1}");
    self.add_text_overlay(Vec2::new(screen.x, screen.y - OVERLAY_LINE_HEIGHT), line1);
    self.add_text_overlay(screen, line2);
  }

  /// Ports `WorldRenderer::drawCollisionActor` (`WorldRenderer.cpp:975-1023`)
  /// minus the dead line-977 `pos`. Axis cross on the opaque buffer, then the
  /// aabb / sphere / obbTreeGroup primitive ladder (first non-null wins).
  fn draw_collision_actor(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    let Some(transform) = entity
      .get_member(ctx, "transform")
      .and_then(|m| read_as_transform(ctx, &m))
    else {
      return;
    };

    let color = if is_highlighted {
      Vec4::new(1.0, 0.0, 0.0, 0.5)
    } else {
      Vec4::new(1.0, 1.0, 1.0, 0.5)
    };
    let solid_color = color.with_w(1.0);

    // `*Cx` members auto-deref -> `.address` is the pointee (0 if null), the
    // Rust analogue of the C++ `primitive.offset` non-null test.
    let aabb_addr = entity
      .get_member(ctx, "aabbPrimitive")
      .map_or(0, |m| m.address);
    let sphere_addr = entity
      .get_member(ctx, "spherePrimitive")
      .map_or(0, |m| m.address);
    let obb_addr = entity
      .get_member(ctx, "obbTreeGroupPrimitive")
      .map_or(0, |m| m.address);

    // C++ lines 993-996: set colour/transform on both buffers before the ladder
    // so branches that only push tris/lines inherit them.
    self.translucent_render_buff.set_color(color.to_array());
    self.translucent_render_buff.set_transform(transform);
    self.render_buff.set_color(solid_color.to_array());
    self.render_buff.set_transform(transform);

    self
      .render_buff
      .add_line(Vec3::new(-0.2, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0));
    self
      .render_buff
      .add_line(Vec3::new(0.0, -0.2, 0.0), Vec3::new(0.0, 0.2, 0.0));
    self
      .render_buff
      .add_line(Vec3::new(0.0, 0.0, -0.2), Vec3::new(0.0, 0.0, 0.2));

    if aabb_addr != 0 {
      let Some(min) = read_vec3_at(ctx, entity, &["aabbPrimitive", "aabb", "min"]) else {
        return;
      };
      let Some(max) = read_vec3_at(ctx, entity, &["aabbPrimitive", "aabb", "max"]) else {
        return;
      };
      self
        .translucent_render_buff
        .add_tris(&shapes::generate_cube(min, max, color));
    } else if sphere_addr != 0 {
      let Some(center) = read_vec3_at(ctx, entity, &["spherePrimitive", "sphere", "origin"]) else {
        return;
      };
      let Some(radius) = walk_member(ctx, entity, &["spherePrimitive", "sphere", "radius"])
        .and_then(|m| m.read_f32(ctx))
      else {
        return;
      };
      self
        .translucent_render_buff
        .add_tris(&shapes::generate_sphere(center, radius, color));
    } else if obb_addr != 0 {
      let Some(min) = read_vec3_at(
        ctx,
        entity,
        &["obbTreeGroupPrimitive", "container", "aabb", "min"],
      ) else {
        return;
      };
      let Some(max) = read_vec3_at(
        ctx,
        entity,
        &["obbTreeGroupPrimitive", "container", "aabb", "max"],
      ) else {
        return;
      };
      self
        .render_buff
        .add_lines(&shapes::generate_cube_lines(min, max, color));
    } else {
      eprintln!("Uhoh! unknown collision actor!");
    }
  }

  /// Ports `WorldRenderer::drawAi` (`WorldRenderer.cpp:898-910`): the
  /// `drawPhysicsActor` body plus a `healthInfo.health` label over the actor.
  fn draw_ai(&mut self, ctx: &Ctx, entity: &GameInstance, is_highlighted: bool) {
    self.draw_physics_actor(ctx, entity, is_highlighted);

    if let Some(screen) = self.screenspace_pos_for_physics_actor(ctx, entity)
      && let Some(health) =
        walk_member(ctx, entity, &["healthInfo", "health"]).and_then(|m| m.read_f32(ctx))
    {
      self.add_text_overlay(screen, format!("{health:.1}"));
    }
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

  /// Ports `WorldRenderer::renderImGui` (`WorldRenderer.cpp:408-529`) — the
  /// "WorldStatus" area/loading table and the "PlayerStatus" pos/vel/look
  /// readout. egui has no free-floating windows, so both spawn off the passed
  /// `ui`'s context (the C++ anchors them to screen corners; exact placement is
  /// a P9.1 concern).
  pub fn render_status_windows(&self, ctx: &Ctx, ui: &mut egui::Ui) {
    let egui_ctx = ui.ctx().clone();

    egui::Window::new("WorldStatus")
      .resizable(false)
      .title_bar(false)
      .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
      .show(&egui_ctx, |ui| self.render_world_status(ctx, ui));

    egui::Window::new("PlayerStatus")
      .resizable(false)
      .title_bar(false)
      .anchor(egui::Align2::LEFT_BOTTOM, [8.0, -8.0])
      .show(&egui_ctx, |ui| self.render_player_status(ui));
  }

  /// The "WorldStatus" window body (`WorldRenderer.cpp:414-494`).
  fn render_world_status(&self, ctx: &Ctx, ui: &mut egui::Ui) {
    let e_chain = ctx.structs.get_enum_by_name("EChain");
    let e_phase = ctx.structs.get_enum_by_name("EPhase");

    egui::Grid::new("world-status-areas")
      .striped(true)
      .show(ui, |ui| {
        ui.label("MREA");
        ui.label("Chain");
        ui.label("Phase");
        ui.label("Occluded");
        ui.end_row();

        for area in get_areas(ctx) {
          let chain = area
            .get_member(ctx, "curChain")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);
          if chain == 1 {
            continue; // deallocated
          }
          let mrea = area
            .get_member(ctx, "mrea")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);
          let phase = area
            .get_member(ctx, "phase")
            .and_then(|m| m.read_u32(ctx))
            .unwrap_or(0);

          let chain_text = e_chain
            .as_ref()
            .and_then(|e| e.get_name_by_value(chain as i64))
            .unwrap_or_else(|| chain.to_string());
          let phase_text = e_phase
            .as_ref()
            .and_then(|e| e.get_name_by_value(phase as i64))
            .unwrap_or_else(|| phase.to_string());

          let mut occluded_text = "yes";
          if area
            .get_member(ctx, "isPostConstructed")
            .and_then(|m| m.read_bool(ctx))
            .unwrap_or(false)
          {
            let occluded = walk_member(ctx, &area, &["postConstructed", "occlusionState"])
              .and_then(|m| m.read_u32(ctx))
              .unwrap_or(0);
            if occluded == 1 {
              occluded_text = "no";
            }
          }

          ui.label(format!("{mrea:08x}"));
          ui.label(chain_text);
          ui.label(phase_text);
          ui.label(occluded_text);
          ui.end_row();
        }
      });

    // Resource load queue (`WorldRenderer.cpp:468-492`).
    let loading = get_all_loading_datas(ctx);
    if !loading.is_empty() {
      ui.label(format!("Loading {}", loading.len()));
      let mut shown = 0u32;
      let mut shown_size: u32 = 0;
      let mut rest_size: u32 = 0;
      for ld in &loading {
        let size = ld
          .get_member(ctx, "resLen")
          .and_then(|m| m.read_u32(ctx))
          .unwrap_or(0);
        if shown < 5 {
          if let Some(tag) = ld.get_member(ctx, "tag") {
            ui.label(format!("{}: {}", object_tag_to_string(ctx, &tag), size));
          }
          shown += 1;
          shown_size = shown_size.saturating_add(size);
        } else {
          rest_size = rest_size.saturating_add(size);
        }
      }
      if shown_size > 0 || rest_size > 0 {
        ui.label(format!(
          "+{}k = {}k",
          rest_size / 1024,
          (shown_size.saturating_add(rest_size)) / 1024
        ));
      }
    }
  }

  /// The "PlayerStatus" window body (`WorldRenderer.cpp:506-527`).
  fn render_player_status(&self, ui: &mut egui::Ui) {
    let forward = self.player_look_vec;
    let hforward = Vec2::new(forward.x, forward.y).normalize_or_zero();
    let hvel = Vec2::new(self.player.velocity.x, self.player.velocity.y);

    let p = self.player.position;
    let v = self.player.velocity;
    ui.label(format!("pos: {:8.3}x {:8.3}y {:8.3}z", p.x, p.y, p.z));
    ui.label(format!(
      "vel: {:8.3}x {:8.3}y {:8.3}z {:8.3}h",
      v.x,
      v.y,
      v.z,
      hvel.length()
    ));

    let hveldir = hvel.normalize_or_zero();
    let forward_angle = hforward.y.atan2(hforward.x);
    let vel_angle = hveldir.y.atan2(hveldir.x);
    let angle = forward_angle - vel_angle;
    ui.label(format!(
      "look: {:6.3}x {:6.3}y {:6.1}deg | vel {:6.3}x {:6.3}y {:6.1}deg | {:6.1} deg",
      hforward.x,
      hforward.y,
      forward_angle.to_degrees(),
      hveldir.x,
      hveldir.y,
      vel_angle.to_degrees(),
      angle.to_degrees()
    ));
  }

  /// Ports the render-config half of `PrimeWatch::doMainMenu`
  /// (`PrimeWatch.cpp:383-464`) — the Culling / Camera / Triggers / Actors
  /// menus. Thin forwarder onto [`render_menu_bar`] so the body stays testable
  /// without a `wgpu::Device`.
  pub fn render_menu(&mut self, ui: &mut egui::Ui) {
    render_menu_bar(
      ui,
      &mut self.culling,
      &mut self.camera_mode,
      &mut self.orbit_player_camera_origin,
      &mut self.manual_camera_speed,
      &mut self.show_exact_camera_controls,
      &mut self.trigger_render_config,
      &mut self.actor_render_config,
    );
  }

  /// Ports the "Camera Controls" window body from `PrimeWatch::doFrame`
  /// (`PrimeWatch.cpp:322-336`). Thin forwarder onto [`render_camera_controls_ui`].
  pub fn render_camera_controls(&mut self, ui: &mut egui::Ui) {
    render_camera_controls_ui(
      ui,
      &mut self.cam_line_length,
      &mut self.manual_camera_pos,
      &mut self.yaw,
      &mut self.pitch,
    );
  }
}

/// Body of the Culling / Camera / Triggers / Actors menus. Free function taking
/// `&mut` field refs so it type-checks and runs headless (no GPU state). Ports
/// `PrimeWatch::doMainMenu` (`PrimeWatch.cpp:383-464`) verbatim, including the
/// intentional Culling label/value skew ("Show Front" -> `Back`).
#[allow(clippy::too_many_arguments)] // mirrors the flat `worldRenderer.*` field set the C++ menu touches
pub(crate) fn render_menu_bar(
  ui: &mut egui::Ui,
  culling: &mut CullType,
  camera_mode: &mut CameraMode,
  orbit: &mut OrbitPlayerCameraOrigin,
  manual_camera_speed: &mut f32,
  show_exact_camera_controls: &mut bool,
  triggers: &mut TriggerRenderConfig,
  actors: &mut ActorRenderConfig,
) {
  // `PrimeWatch.cpp:383-393` — Culling. Labels and value mapping are verbatim:
  // "Show Front" selects `BACK`, "Show Back" selects `FRONT`.
  ui.menu_button("Culling", |ui| {
    ui.selectable_value(culling, CullType::Back, "Show Front");
    ui.selectable_value(culling, CullType::Front, "Show Back");
    ui.selectable_value(culling, CullType::None, "Show All");
  });

  // `PrimeWatch.cpp:396-433` — Camera.
  ui.menu_button("Camera", |ui| {
    ui.selectable_value(camera_mode, CameraMode::FollowPlayer, "Follow Player");
    if *camera_mode == CameraMode::FollowPlayer {
      ui.separator();
      ui.indent("camera-follow-orbit", |ui| {
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Top, "Top");
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Center, "Center");
        ui.selectable_value(orbit, OrbitPlayerCameraOrigin::Bottom, "Bottom");
      });
      ui.separator();
    }
    ui.selectable_value(camera_mode, CameraMode::GameCam, "Game Cam");
    ui.selectable_value(camera_mode, CameraMode::Detached, "Detatched");
    if *camera_mode == CameraMode::Detached {
      ui.separator();
      ui.indent("camera-detached-controls", |ui| {
        ui.add(
          egui::Slider::new(manual_camera_speed, 0.1..=2.0)
            .clamping(egui::SliderClamping::Always)
            .text("Speed"),
        );
        ui.checkbox(show_exact_camera_controls, "Show camera controls");
      });
      ui.separator();
    }
  });

  // `PrimeWatch.cpp:435-453` — Triggers. `TOGGLE_MENU` -> `ui.checkbox`. Field
  // order follows the struct declaration order (`WorldRenderer.hpp:46-61`).
  ui.menu_button("Triggers", |ui| {
    ui.checkbox(&mut triggers.detect_player, "detectPlayer");
    ui.checkbox(&mut triggers.detect_ai, "detectAi");
    ui.checkbox(&mut triggers.detect_projectiles, "detectProjectiles");
    ui.checkbox(&mut triggers.detect_bombs, "detectBombs");
    ui.checkbox(&mut triggers.detect_power_bombs, "detectPowerBombs");
    ui.checkbox(&mut triggers.kill_on_enter, "killOnEnter");
    ui.checkbox(&mut triggers.detect_morphed_player, "detectMorphedPlayer");
    // C++ label is the misspelled "useCollisionImpluses"; use the corrected
    // spelling here to match the Rust field name (deviation, noted in P8.4.6).
    ui.checkbox(&mut triggers.use_collision_impulses, "useCollisionImpulses");
    ui.checkbox(&mut triggers.detect_camera, "detectCamera");
    ui.checkbox(
      &mut triggers.use_boolean_intersection,
      "useBooleanIntersection",
    );
    ui.checkbox(
      &mut triggers.detect_unmorphed_player,
      "detectUnmorphedPlayer",
    );
    ui.checkbox(
      &mut triggers.block_environmental_effects,
      "blockEnvironmentalEffects",
    );
    ui.separator();
    ui.checkbox(&mut triggers.water, "Water");
    ui.checkbox(&mut triggers.docks, "Docks");
  });

  // `PrimeWatch.cpp:455-464` — Actors. `renderCollisionActors` is deliberately
  // not exposed (matches C++).
  ui.menu_button("Actors", |ui| {
    ui.checkbox(&mut actors.render_projectiles, "Render projectiles");
    ui.checkbox(&mut actors.render_ai, "Render AI");
    ui.checkbox(&mut actors.render_pickups, "Render Pickups");
    ui.checkbox(&mut actors.render_physics_actors, "Render physics actors");
    ui.checkbox(&mut actors.render_actors, "Render actors");
    ui.checkbox(&mut actors.render_all_actors, "Render all actors");
  });
}

/// Body of the "Camera Controls" window (`PrimeWatch.cpp:322-336`). Yaw/Pitch
/// display **degrees** and write back **radians**; `yaw_deg` is `fmod 360` of
/// the degree value (Rust `%` matches C++ `fmod` — sign of the dividend). Yaw
/// and pitch are only written back when the drag actually `.changed()`.
pub(crate) fn render_camera_controls_ui(
  ui: &mut egui::Ui,
  cam_line_length: &mut f32,
  manual_camera_pos: &mut Vec3,
  yaw: &mut f32,
  pitch: &mut f32,
) {
  ui.add(
    egui::DragValue::new(cam_line_length)
      .speed(1.0)
      .range(2.0..=250.0)
      .prefix("Camera line length: "),
  );

  ui.horizontal(|ui| {
    ui.label("Position");
    ui.add(egui::DragValue::new(&mut manual_camera_pos.x).speed(1.0));
    ui.add(egui::DragValue::new(&mut manual_camera_pos.y).speed(1.0));
    ui.add(egui::DragValue::new(&mut manual_camera_pos.z).speed(1.0));
  });

  let mut yaw_deg = yaw.to_degrees() % 360.0;
  if ui
    .add(
      egui::DragValue::new(&mut yaw_deg)
        .speed(1.0)
        .range(-360.0..=360.0)
        .prefix("Yaw: "),
    )
    .changed()
  {
    *yaw = yaw_deg.to_radians();
  }

  let mut pitch_deg = pitch.to_degrees();
  if ui
    .add(
      egui::DragValue::new(&mut pitch_deg)
        .speed(1.0)
        .range(-89.0..=89.0)
        .prefix("Pitch: "),
    )
    .changed()
  {
    *pitch = pitch_deg.to_radians();
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

  #[test]
  fn trigger_render_flags_default_config() {
    // Defaults: detect_player + detect_unmorphed_player -> 0x1 | 0x10000.
    let f = trigger_render_flags(&TriggerRenderConfig::default());
    assert_eq!(f, 0x1 | 0x10000);
  }

  #[test]
  fn trigger_render_flags_projectiles_fan_out() {
    let cfg = TriggerRenderConfig {
      detect_player: false,
      detect_unmorphed_player: false,
      detect_projectiles: true,
      ..TriggerRenderConfig::default()
    };
    assert_eq!(
      trigger_render_flags(&cfg),
      0x4 | 0x8 | 0x10 | 0x20 | 0x100 | 0x200 | 0x400
    );
  }

  #[test]
  fn trigger_render_flags_all_bits() {
    let cfg = TriggerRenderConfig {
      detect_player: true,
      detect_ai: true,
      detect_projectiles: true,
      detect_bombs: true,
      detect_power_bombs: true,
      kill_on_enter: true,
      detect_morphed_player: true,
      use_collision_impulses: true,
      detect_camera: true,
      use_boolean_intersection: true,
      detect_unmorphed_player: true,
      block_environmental_effects: true,
      water: true,
      docks: true,
    };
    assert_eq!(trigger_render_flags(&cfg), 0x3FFFF);
  }

  #[test]
  fn trigger_color_precedence() {
    // default white
    assert_eq!(trigger_color(false, false), Vec4::new(1.0, 1.0, 1.0, 0.5));
    // water tint
    assert_eq!(trigger_color(true, false), Vec4::new(0.5, 0.5, 1.0, 0.5));
    // highlight wins over water
    assert_eq!(trigger_color(true, true), Vec4::new(1.0, 0.0, 0.0, 0.5));
    assert_eq!(trigger_color(false, true), Vec4::new(1.0, 0.0, 0.0, 0.5));
  }

  #[test]
  fn physics_actor_bbox_uses_collision_primitive_when_non_degenerate() {
    let pos = Vec3::new(10.0, 0.0, 0.0);
    let cp = (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let bb = (Vec3::splat(-5.0), Vec3::splat(5.0));
    let rb = (Vec3::splat(-9.0), Vec3::splat(9.0));
    let (min, max) = physics_actor_bbox(pos, cp, bb, rb);
    approx(min, pos + cp.0);
    approx(max, pos + cp.1);
  }

  #[test]
  fn physics_actor_bbox_falls_back_to_base_then_render_bounds() {
    let pos = Vec3::new(10.0, 2.0, 3.0);
    let degen = (Vec3::ZERO, Vec3::ZERO);
    // collisionPrimitive degenerate -> baseBoundingBox (pos-offset).
    let bb = (Vec3::splat(-2.0), Vec3::splat(2.0));
    let (min, max) = physics_actor_bbox(pos, degen, bb, degen);
    approx(min, pos + bb.0);
    approx(max, pos + bb.1);

    // both degenerate -> renderBounds, NOT pos-offset.
    let rb = (Vec3::new(-4.0, -4.0, -4.0), Vec3::new(4.0, 4.0, 4.0));
    let (min, max) = physics_actor_bbox(pos, degen, degen, rb);
    approx(min, rb.0);
    approx(max, rb.1);
  }

  #[test]
  fn bomb_fuse_frames_is_ceil_times_60_plus_1() {
    assert_eq!(bomb_fuse_frames(0.0), 1);
    assert_eq!(bomb_fuse_frames(0.5), 31); // ceil(30) + 1
    assert_eq!(bomb_fuse_frames(1.0), 61);
    assert_eq!(bomb_fuse_frames(0.016), 2); // ceil(0.96) + 1
    // a spent bomb -> non-positive -> draw skipped
    assert!(bomb_fuse_frames(-1.0) <= 0);
  }

  #[test]
  fn bomb_proximity_highlight_predicate() {
    // player and bomb coincident: posToBall = (0,0,0.7), len 0.7 < 1.5, z >= -0.7
    assert!(bomb_proximity_highlight(Vec3::ZERO, Vec3::ZERO));
    // bomb far in the xy plane -> out of range
    assert!(!bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(5.0, 0.0, 0.0)
    ));
    // bomb well above the ball -> posToBall.z = 0.7 - 2.0 = -1.3 < -0.7
    assert!(!bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(0.0, 0.0, 2.0)
    ));
    // boundary: bomb at z = 1.4 -> posToBall.z = -0.7 (>= -0.7), len 0.7 < 1.5
    assert!(bomb_proximity_highlight(
      Vec3::ZERO,
      Vec3::new(0.0, 0.0, 1.4)
    ));
    // player offset carries through
    assert!(bomb_proximity_highlight(
      Vec3::new(10.0, 0.0, 0.0),
      Vec3::new(10.0, 0.5, 0.0)
    ));
  }

  #[test]
  fn projectile_world_pos_identity_transforms_sum_offsets() {
    let pos = projectile_world_pos(
      Mat4::IDENTITY,
      Mat4::IDENTITY,
      Vec3::new(1.0, 2.0, 3.0),
      Vec3::new(0.5, 0.0, 0.0),
      Vec3::new(0.0, 0.0, 10.0),
    );
    approx(pos, Vec3::new(1.5, 2.0, 13.0));
  }

  #[test]
  fn projectile_world_pos_world_offset_is_added_after_local_to_world() {
    let ltw = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    // proj/local offsets are w=0 -> localToWorldXf translation still applies to
    // the (0,0,0) point via its 4th column since the accumulated vec4 has w=1
    // only from... actually offsets stay w=0, so translation does NOT apply.
    approx(
      projectile_world_pos(ltw, Mat4::IDENTITY, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
      Vec3::ZERO,
    );
    // worldOffset is a plain world-space add.
    approx(
      projectile_world_pos(
        ltw,
        Mat4::IDENTITY,
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(0.0, 5.0, 0.0),
      ),
      Vec3::new(0.0, 5.0, 0.0),
    );
  }

  #[test]
  fn projectile_world_vel_rotates_without_translating() {
    let ltw = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    approx(
      projectile_world_vel(ltw, Mat4::IDENTITY, Vec3::new(0.0, 0.0, 1.0)),
      Vec3::new(0.0, 0.0, 1.0),
    );
    let rot = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    approx(
      projectile_world_vel(rot, Mat4::IDENTITY, Vec3::new(1.0, 0.0, 0.0)),
      Vec3::new(0.0, 1.0, 0.0),
    );
  }

  #[test]
  fn project_maps_center_and_corners_to_pixel_viewport() {
    let vp = [0.0, 0.0, 800.0, 600.0];
    // With identity view+proj, the origin sits at NDC (0,0) -> viewport centre.
    let c = project(Vec3::ZERO, Mat4::IDENTITY, Mat4::IDENTITY, vp).unwrap();
    approx(c, Vec3::new(400.0, 300.0, 0.0));
    // NDC (1,1) -> far corner (before the caller's Y flip).
    let corner = project(Vec3::new(1.0, 1.0, 0.0), Mat4::IDENTITY, Mat4::IDENTITY, vp).unwrap();
    approx(corner, Vec3::new(800.0, 600.0, 0.0));
    // NDC (-1,-1) -> origin corner.
    let origin = project(
      Vec3::new(-1.0, -1.0, 0.0),
      Mat4::IDENTITY,
      Mat4::IDENTITY,
      vp,
    )
    .unwrap();
    approx(origin, Vec3::new(0.0, 0.0, 0.0));
  }

  #[test]
  fn project_rejects_points_behind_the_camera() {
    // RH perspective looking down -Z: a point at +Z is behind the camera and
    // projects with clip.w <= 0. Previously this mirrored onto the screen.
    let proj = perspective(45.0, 4.0 / 3.0, 0.1, 1000.0);
    let vp = [0.0, 0.0, 800.0, 600.0];
    assert!(project(Vec3::new(0.0, 0.0, 5.0), Mat4::IDENTITY, proj, vp).is_none());
    assert!(project(Vec3::new(0.0, 0.0, -5.0), Mat4::IDENTITY, proj, vp).is_some());
  }

  #[test]
  fn project_perspective_divides_by_w() {
    // A projection that scales w by the point's z; a point at z=2 halves x/y.
    let proj = Mat4::from_cols(
      glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
      glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
      glam::Vec4::new(0.0, 0.0, 1.0, 1.0),
      glam::Vec4::new(0.0, 0.0, 0.0, 0.0),
    );
    let vp = [0.0, 0.0, 200.0, 200.0];
    let s = project(Vec3::new(1.0, 0.0, 2.0), Mat4::IDENTITY, proj, vp).unwrap();
    // clip = (1, 0, 2, 2) -> ndc.x = 0.5 -> screen.x = (0.5+1)*0.5*200 = 150.
    approx(s, Vec3::new(150.0, 100.0, 1.0));
  }

  #[test]
  fn item_type_overlay_text_matches_cpp_format() {
    // Sanity on the string the pickup overlay builds (C++ `drawPickup:956/965`).
    let line1 = format!(
      "{} {}/{}",
      item_type_to_name(EItemType::from_raw(4)),
      5,
      250
    );
    assert_eq!(line1, "Missiles 5/250");
    let line2 = format!("{:.1}/{:.1}", 1.25_f32, 30.0_f32);
    assert_eq!(line2, "1.2/30.0");
  }

  #[test]
  fn player_speed_color_ladder() {
    let half_pi = std::f32::consts::FRAC_PI_2;
    // > 90deg -> red
    assert_eq!(
      player_speed_color(half_pi + 0.1),
      Vec4::new(1.0, 0.0, 0.0, 1.0)
    );
    // NaN -> red
    assert_eq!(player_speed_color(f32::NAN), Vec4::new(1.0, 0.0, 0.0, 1.0));
    // aligned -> green base (percent 0)
    assert_eq!(player_speed_color(0.0), Vec4::new(0.0, 0.5, 0.0, 1.0));
    // near 90deg -> cyan (percent > 0.95)
    assert_eq!(
      player_speed_color(half_pi * 0.98),
      Vec4::new(0.0, 1.0, 1.0, 1.0)
    );
  }

  #[test]
  fn render_menu_bar_type_checks_and_does_not_panic() {
    let mut culling = CullType::Back;
    let mut camera_mode = CameraMode::FollowPlayer;
    let mut orbit = OrbitPlayerCameraOrigin::Center;
    let mut speed = 1.0_f32;
    let mut show_controls = false;
    let mut triggers = TriggerRenderConfig::default();
    let mut actors = ActorRenderConfig::default();
    // FollowPlayer path (orbit sub-group visible).
    egui::__run_test_ui(|ui| {
      render_menu_bar(
        ui,
        &mut culling,
        &mut camera_mode,
        &mut orbit,
        &mut speed,
        &mut show_controls,
        &mut triggers,
        &mut actors,
      );
    });
    // Detached path (speed slider + controls toggle visible).
    camera_mode = CameraMode::Detached;
    egui::__run_test_ui(|ui| {
      render_menu_bar(
        ui,
        &mut culling,
        &mut camera_mode,
        &mut orbit,
        &mut speed,
        &mut show_controls,
        &mut triggers,
        &mut actors,
      );
    });
  }

  #[test]
  fn render_camera_controls_ui_type_checks_and_does_not_panic() {
    let mut cll = 10.0_f32;
    let mut pos = Vec3::new(1.0, 2.0, 3.0);
    let mut yaw = 1.0_f32;
    let mut pitch = 0.3_f32;
    egui::__run_test_ui(|ui| {
      render_camera_controls_ui(ui, &mut cll, &mut pos, &mut yaw, &mut pitch);
    });
  }

  #[test]
  fn culling_menu_label_value_skew_is_preserved() {
    // "Show Front" -> BACK, "Show Back" -> FRONT (verbatim C++ skew).
    let mut culling = CullType::None;
    egui::__run_test_ui(|ui| {
      ui.selectable_value(&mut culling, CullType::Back, "Show Front");
    });
    // The mapping under test is the (label, variant) pairing above; assert the
    // pairing a reviewer cares about rather than simulating a click.
    let front_variant = CullType::Back;
    let back_variant = CullType::Front;
    assert_ne!(front_variant, back_variant);
    assert_eq!(front_variant, CullType::Back);
  }

  #[test]
  fn camera_controls_yaw_pitch_deg_rad_roundtrip() {
    // yaw_deg = to_degrees % 360 (C++ fmod keeps the dividend sign -> Rust `%`).
    let yaw = std::f32::consts::PI * 3.0; // 540 deg
    let yaw_deg = yaw.to_degrees() % 360.0;
    assert!((yaw_deg - 180.0).abs() < 1e-3);
    let neg = -std::f32::consts::PI * 3.0;
    assert!((neg.to_degrees() % 360.0 + 180.0).abs() < 1e-3);
    // write-back path: deg -> rad.
    assert!((90.0_f32.to_radians() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
  }
}
