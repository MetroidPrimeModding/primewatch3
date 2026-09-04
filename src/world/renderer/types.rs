//! Plain data / config types shared across the renderer submodules: camera
//! mode enums, the `GameCamera` / `WorldInput` snapshots, the per-feature
//! render-config toggle structs, and the screen-space text overlay.

use glam::{Vec2, Vec3};

/// Selected by the Culling menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CullType {
  Back,
  Front,
  None,
}

/// Selected by the Camera menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
  FollowPlayer,
  Detached,
  GameCam,
}

/// Selected by the Camera menu (Follow Player sub-group).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrbitPlayerCameraOrigin {
  Top,
  Center,
  Bottom,
}

/// The in-game camera as read from `CGameCamera`. Only `perspective` /
/// `transform` are consumed; `fov` / `znear` / `zfar` / `aspect` are read for
/// the camera status UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameCamera {
  pub perspective: glam::Mat4,
  pub transform: glam::Mat4,
  // shown by the camera status window
  pub fov: f32,

  pub znear: f32,

  pub zfar: f32,

  pub aspect: f32,
}

/// The camera-motion inputs (`PrimeWatchInput` minus `capturedMouse`). `app.rs`
/// passes [`WorldInput::default`] (all zero — no camera motion) until real winit
/// plumbing lands.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldInput {
  pub cam_pitch: f32,
  pub cam_yaw: f32,
  pub cam_zoom: f32,
}

/// `enabled` gates the `player_ghosts` draw loop; nothing populates the ghost
/// array yet (matches C++ — the loop is a no-op until something feeds it).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerGhost {
  pub enabled: bool,
  pub position: Vec3,
  pub orientation: glam::Quat,
  pub velocity: Vec3,
  pub is_morphed: bool,
}

/// The C++ type is a packed bitfield with per-field initializers; here it's
/// plain `bool`s with a matching `Default`. The UI toggles these.
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

/// Same bitfield-to-`bool` treatment as [`TriggerRenderConfig`].
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

/// The `fs_mesh` "hide geometry between the camera and the player" cutout — a
/// bayer dissolve inside a cone with its apex at the camera and its axis on the
/// player, ending in a rounded hemisphere cap rather than a flat disc. Tuned
/// from the Culling menu, marshalled into `WorldUniforms::clip_params`.
#[derive(Clone, Copy, Debug)]
pub struct PlayerClipConfig {
  /// Feature toggle.
  pub enabled: bool,
  /// Cone radius at the hemisphere cap, in world units.
  pub cone_radius: f32,
  /// World-unit slack in front of the player before the hemisphere cap starts.
  pub player_margin: f32,
  /// World-unit width of the soft edge on the cone/hemisphere radius cutoff.
  pub player_fade: f32,
  /// Lower bound on how faint dissolved geometry gets (`0.0` = can vanish
  /// entirely, `0.3` = never less than ~30% of pixels kept).
  pub min_visibility: f32,
}

impl Default for PlayerClipConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      cone_radius: 2.0,
      player_margin: 2.5,
      player_fade: 1.0,
      min_visibility: 0.45,
    }
  }
}

/// A screen-space text label accumulated during `update` and painted by the app
/// shell's overlay pass. From the `ImDrawList::AddText` calls in the per-class
/// draw functions.
#[derive(Clone, Debug, PartialEq)]
pub struct TextOverlay {
  pub screen_pos: Vec2,
  pub text: String,
}

/// Nominal line height for stacking multi-line overlays (`drawPickup`'s two
/// lines). The C++ uses `ImGui::GetTextLineHeight()`; this layer has no font
/// system, so the overlay painter owns exact glyph metrics / horizontal
/// centering.
pub(crate) const OVERLAY_LINE_HEIGHT: f32 = 14.0;
