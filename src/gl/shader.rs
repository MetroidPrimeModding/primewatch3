//! `WorldUniforms` + WGSL + `WorldPipelines` — the uniform block, the world
//! shader, and the render pipelines (opaque + translucent, mesh + line), plus
//! the per-frame uniform upload.

use crate::gl::{Vert, as_bytes};
use glam::{Mat4, Vec3A, Vec4};

/// Uniforms poked by `meshShader->setMat4` / `setVec3` in
/// `WorldRenderer::render`: `model`, `view`, `projection`, `viewPos`, `lightDir`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WorldUniforms {
  pub model: Mat4,
  pub view: Mat4,
  pub projection: Mat4,
  /// `Mat4::from_mat3(Mat3::from_mat4(model).inverse().transpose())`.
  pub normal_matrix: Mat4,
  pub view_pos: Vec3A,
  pub light_dir: Vec3A,
  /// `xyz` = player world position. `w` = clip flag: `1.0` applies the
  /// near-player bayer cutout in `fs_mesh`, `0.0` draws every fragment (used
  /// when rendering the player / ghost models themselves). Packed into `w`
  /// rather than a trailing `u32` so the Rust and WGSL layouts can't disagree
  /// over `vec3` tail padding.
  pub player_pos: Vec4,
  /// Tuning for the `fs_mesh` cutout: `x` = cone radius at the cap (world
  /// units), `y` = player margin, `z` = soft-edge width on the cone/hemisphere
  /// radius cutoff, `w` = feature enabled (`1.0` / `0.0`). Driven by the
  /// Culling menu.
  pub clip_params: Vec4,
  /// More `fs_mesh` cutout tuning: `x` = minimum visibility (`keep` floor, so
  /// dissolved geometry never drops below this fraction), `yzw` reserved.
  /// Driven by the Culling menu.
  pub clip_params2: Vec4,
  /// The player shadow map's `light_view_proj` (light-space clip = this *
  /// world-space frag position). Recomputed every frame from the live player
  /// position in `WorldRenderer::render`, independent of the main camera.
  pub light_view_proj: Mat4,
  /// `fs_mesh` shadow tuning, from `ShadowConfig`: `x` = strength (how much
  /// direct light a shadowed fragment loses), `y` = depth bias, `z` = feature
  /// enabled (`1.0` / `0.0`), `w` reserved.
  pub shadow_params: Vec4,
}

impl Default for WorldUniforms {
  fn default() -> Self {
    let ident = Mat4::IDENTITY;
    Self {
      model: ident,
      view: ident,
      projection: ident,
      normal_matrix: ident,
      view_pos: Vec3A::ZERO,
      light_dir: Vec3A::ZERO,
      player_pos: Vec4::new(0.0, 0.0, 0.0, 1.0),
      clip_params: Vec4::new(1.5, 2.0, 1.0, 1.0),
      clip_params2: Vec4::ZERO,
      light_view_proj: ident,
      shadow_params: Vec4::new(0.6, 0.0015, 0.0, 0.0),
    }
  }
}

/// One WGSL module: `vs_main` (from `meshVertShader`, shared), `fs_mesh` (from
/// `meshFragShader`) and `fs_line` (from `lineFragShader`).
///
/// **Gamma:** the fragment shaders write raw linear values, exactly like the
/// C++ (which rendered into a plain, non-sRGB GL backbuffer). An earlier port
/// applied `linear_to_srgb` here, but egui-wgpu composites the `Rgba8Unorm`
/// world texture through `fs_main_linear_framebuffer` (`linear_from_gamma` on
/// the sample) and the swapchain is an sRGB surface, so that encode became a
/// *second* one on top of the hardware's — the world view came out visibly
/// brighter than the C++. With the raw output, egui's decode and the surface's
/// encode cancel and the texture reaches the screen verbatim.
///
/// The frag shaders use the interpolated `normal` *without* re-normalizing —
/// preserved from the GLSL. Constants (`0.015`, `0.7`, `0.5`, `0.2`, `256`) are
/// kept exact.
pub const WORLD_SHADER_WGSL: &str = r#"
struct Uniforms {
  model: mat4x4<f32>,
  view: mat4x4<f32>,
  projection: mat4x4<f32>,
  normal_matrix: mat4x4<f32>,
  view_pos: vec3<f32>,
  light_dir: vec3<f32>,
  player_pos: vec4<f32>,
  clip_params: vec4<f32>,
  clip_params2: vec4<f32>,
  light_view_proj: mat4x4<f32>,
  shadow_params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_map_sampler: sampler_comparison;

struct VsOut {
  @builtin(position) clip_pos: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) frag_pos: vec3<f32>,
  @location(3) barycentric: vec3<f32>,
};

const BAYER4_LUT = array<u32, 16>(
   0u,  8u,  2u, 10u,
  12u,  4u, 14u,  6u,
   3u, 11u,  1u,  9u,
  15u,  7u, 13u,  5u,
);

fn bayer4x4(in: vec2<u32>) -> f32 {
  let idx = (in.y & 3) * 4 + (in.x & 3);
  return f32(BAYER4_LUT[idx]) / 16.0;
}

@vertex
fn vs_main(@location(0) a_pos: vec3<f32>, @location(1) a_color: vec4<f32>,
           @location(2) a_normal: vec3<f32>, @location(3) a_barycentric: vec3<f32>) -> VsOut {
  var out: VsOut;
  let world = u.model * vec4<f32>(a_pos, 1.0);
  out.clip_pos = u.projection * u.view * world;
  out.color = a_color;
  out.normal = (u.normal_matrix * vec4<f32>(a_normal, 0.0)).xyz;
  out.frag_pos = world.xyz;
  out.barycentric = a_barycentric;
  return out;
}

// Depth-only pass for the player shadow map: rasterizes just the live
// player's opaque model from the light's point of view. Shares `Uniforms`
// (group 0) with `vs_main` / `fs_mesh` so no separate uniform buffer is
// needed -- `light_view_proj` is filled in every frame regardless of which
// pass reads it. `a_color` / `a_normal` / `a_barycentric` are unused here but
// must stay in the signature to match `Vert::LAYOUT`.
@vertex
fn vs_shadow(@location(0) a_pos: vec3<f32>, @location(1) a_color: vec4<f32>,
             @location(2) a_normal: vec3<f32>, @location(3) a_barycentric: vec3<f32>) -> @builtin(position) vec4<f32> {
  return u.light_view_proj * vec4<f32>(a_pos, 1.0);
}

// Samples the player shadow map at world-space `frag_pos`. Returns `1.0`
// (fully lit) when the fragment is outside the light's small ortho frustum --
// the shadow only ever exists in the volume immediately around the player, so
// anything else is untouched. `shadow_map_sampler` is a comparison sampler:
// the raw result is the fraction of samples where `frag_pos` is at least as
// close to the light as the stored (player) depth, i.e. *not* shadowed.
fn player_shadow_factor(frag_pos: vec3<f32>) -> f32 {
  let clip = u.light_view_proj * vec4<f32>(frag_pos, 1.0);
  if (clip.w <= 0.0) {
    return 1.0;
  }
  let light_ndc = clip.xyz / clip.w;
  let uv = vec2<f32>(light_ndc.x * 0.5 + 0.5, 0.5 - light_ndc.y * 0.5);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 ||
      light_ndc.z < 0.0 || light_ndc.z > 1.0) {
    return 1.0;
  }
  let bias = u.shadow_params.y;
  let lit = textureSampleCompare(shadow_map, shadow_map_sampler, uv, light_ndc.z - bias);
  return mix(1.0 - u.shadow_params.x, 1.0, lit);
}

@fragment
fn fs_mesh(in: VsOut) -> @location(0) vec4<f32> {
  let frag_vec = u.view_pos - in.frag_pos;
  let frag_dir = normalize(frag_vec);

  // axis from camera to player
  let axis = u.player_pos.xyz - u.view_pos;
  let axis_len = max(length(axis), 1e-6);
  let axis_dir = axis / axis_len;

  // this fragment as seen from the camera (the cone apex)
  let to_frag = in.frag_pos - u.view_pos;
  let dist_along = dot(to_frag, axis_dir);   // signed distance along the camera->player axis

  // Bayer dissolve of geometry inside a cone whose apex is the camera and whose
  // axis points at the player, ending in a rounded hemisphere cap instead of a
  // flat disc. The cone's lateral surface tapers linearly from radius 0 at the
  // camera to `cone_radius_*` (world units) at `cap_dist` -- `player_margin`
  // world units short of the player, so its feet / a wall behind it are left
  // alone. Past `cap_dist` the radius measure switches from the axis-rescaled
  // cone radius to the raw 3D distance to the cap point, tracing a hemisphere
  // that bulges away from the camera. The two formulas agree exactly at
  // `cap_dist` (the rescale factor is 1 there), so the surface is continuous --
  // no seam where the cone meets the cap.
  let player_margin = u.clip_params.y;
  let cap_dist = max(axis_len - player_margin, 1e-6);
  let dist_along_clamped = min(dist_along, cap_dist);
  let perp = length(to_frag - axis_dir * dist_along_clamped);
  let cone_r = select(perp, perp * cap_dist / max(dist_along, 1e-6), dist_along <= cap_dist);

  let cone_radius_inner = u.clip_params.x;                 // fully inside the cone/cap
  let edge_width = max(u.clip_params.z, 1e-4);             // `player_fade`: soft-edge width
  let cone_radius_outer = cone_radius_inner + edge_width;  // fully outside
  let in_cone = 1.0 - smoothstep(cone_radius_inner, cone_radius_outer, cone_r);

  // Fudge: up-facing floor grazes into the cone far more than walls do, so bias
  // it back toward being kept -- but only for fragments at or below the player's
  // height, so the ground the player stands on is spared while an up-facing
  // platform *above* the player (between it and the camera) still dissolves.
  // `up` is ~1 on a level floor, ~0 on a wall; squared so only near-horizontal
  // surfaces get much help.
  // An idea to explore in the future: maybe we pick if we filter above/below based on the camera orientation?
  let up = clamp(normalize(in.normal).z, 0.0, 1.0);
  let below_player = smoothstep(-0.5, 0.5, u.player_pos.z - in.frag_pos.z);
  let ground_keep_bias = 1 * up * up * below_player;
  // `min_visibility` floors the result so dissolved geometry never fully vanishes.
  let min_visibility = u.clip_params2.x;
  let keep = max(mix(1.0 - in_cone, 1.0, ground_keep_bias), min_visibility);

  let m = bayer4x4(vec2<u32>(in.clip_pos.xy) % 4u);
  if (u.player_pos.w != 0.0 && u.clip_params.w != 0.0 && keep < m) { discard; }
  // if (u.player_pos.w != 0.0) {
  //   return vec4<f32>(keep, m, 0, 1);
  // }

  let edge_thickness = 0.015;
  let min_bary = min(min(in.barycentric.x, in.barycentric.y), in.barycentric.z);
  if (min_bary > 0.0 && min_bary < edge_thickness) {
    return vec4<f32>(vec3<f32>(0.2), 1.0);
  }
  // Player shadow -- only applied to world geometry (`player_pos.w != 0`),
  // never to the player/ghost models themselves (drawn with `bind_group_noclip`,
  // which zeroes `player_pos.w`). Darkens direct light only; ambient is
  // untouched so shadowed ground never goes fully black.
  var shadow = 1.0;
  if (u.player_pos.w != 0.0 && u.shadow_params.z != 0.0) {
    shadow = player_shadow_factor(in.frag_pos);
  }

  let light_color = vec3<f32>(1.0, 1.0, 1.0);
  let ambient = 0.7 * light_color;
  let diff = max(dot(in.normal, u.light_dir), 0.0);
  let diffuse = diff * light_color * 0.5 * shadow;
  let reflect_dir = reflect(-u.light_dir, in.normal);
  let spec = pow(max(dot(frag_dir, reflect_dir), 0.0), 256.0);
  let specular = 0.2 * spec * light_color * shadow;
  let lit = vec4<f32>(ambient + diffuse + specular, 1.0) * in.color;
  return lit;
}

@fragment
fn fs_line(in: VsOut) -> @location(0) vec4<f32> {
  return vec4<f32>(1.0, 1.0, 1.0, 1.0) * in.color;
}
"#;

impl WorldUniforms {
  /// Build the uniform block from the matrices the caller already has, filling
  /// `normal_matrix` from `model`. `light_dir` is taken as-is (caller normalizes).
  pub fn from_matrices(
    model: Mat4,
    view: Mat4,
    projection: Mat4,
    view_pos: glam::Vec3,
    light_dir: glam::Vec3,
    player_pos: glam::Vec3,
  ) -> Self {
    let normal_matrix = Mat4::from_mat3(glam::Mat3::from_mat4(model).inverse().transpose());
    Self {
      model,
      view,
      projection,
      normal_matrix,
      view_pos: view_pos.into(),
      light_dir: light_dir.into(),
      player_pos: player_pos.extend(1.0),
      // Overwritten per-frame from `PlayerClipConfig` / `ShadowConfig` in
      // `WorldRenderer::render`.
      clip_params: Vec4::new(1.5, 2.0, 1.0, 1.0),
      clip_params2: Vec4::ZERO,
      light_view_proj: Mat4::IDENTITY,
      shadow_params: Vec4::new(0.6, 0.0015, 0.0, 0.0),
    }
  }
}

/// Cull-mode order for the mesh-pipeline arrays: `[0]` = no culling, `[1]` =
/// back-face, `[2]` = front-face. Mirrors the `switch (culling)` in
/// `WorldRenderer` — a variant is selected per `CullType` at draw
/// (`front_face: Cw` is baked; lines are never culled).
pub const CULL_MODES: [Option<wgpu::Face>; 3] =
  [None, Some(wgpu::Face::Back), Some(wgpu::Face::Front)];

/// Resolution of the player shadow map. Small on purpose -- it only ever
/// rasterizes one box/sphere-ish player model into a tight frustum around it,
/// not the whole scene, so this doesn't need to be large to look sharp.
const SHADOW_MAP_SIZE: u32 = 1024;

/// Depth-only format for the shadow map texture; `Depth32Float` is sampled as
/// `texture_depth_2d` with a comparison sampler in `fs_mesh`.
const SHADOW_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The mesh + line pipelines (opaque + translucent variants) plus the shared
/// uniform buffer / bind group
///
/// The two mesh pipelines exist in all three [`CULL_MODES`] variants
/// pick one with [`WorldPipelines::mesh`].
pub struct WorldPipelines {
  pub uniform_buffer: wgpu::Buffer,
  pub bind_group: wgpu::BindGroup,
  /// Same uniforms as `bind_group` but with `player_pos.w = 0` — bind this
  /// while drawing the player / ghost models so `fs_mesh` never discards them.
  uniform_buffer_noclip: wgpu::Buffer,
  pub bind_group_noclip: wgpu::BindGroup,
  mesh_opaque: [wgpu::RenderPipeline; 3],
  mesh_translucent: [wgpu::RenderPipeline; 3],
  pub line_opaque: wgpu::RenderPipeline,
  pub line_translucent: wgpu::RenderPipeline,
  /// Depth-only pipeline for the player shadow map (`vs_shadow`, no fragment
  /// stage). Uses its own (shorter) pipeline layout -- group 1 (the shadow
  /// texture itself) is meaningless while rendering *into* it.
  shadow_pipeline: wgpu::RenderPipeline,
  shadow_view: wgpu::TextureView,
  /// Group 1 for every `mesh_*` / `line_*` pipeline: the shadow map texture +
  /// comparison sampler `fs_mesh` reads via `player_shadow_factor`.
  pub shadow_tex_bind_group: wgpu::BindGroup,
}

impl WorldPipelines {
  pub fn new(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
  ) -> Self {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("world-shader"),
      source: wgpu::ShaderSource::Wgsl(WORLD_SHADER_WGSL.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("world-bgl"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });

    // Group 1: the shadow map texture + comparison sampler, read by `fs_mesh`
    // only. Kept as its own group (rather than folded into group 0) so the
    // depth-only shadow pipeline's layout can omit it entirely.
    let shadow_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow-bgl"),
        entries: &[
          wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              sample_type: wgpu::TextureSampleType::Depth,
              view_dimension: wgpu::TextureViewDimension::D2,
              multisampled: false,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
            count: None,
          },
        ],
      });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("world-pl"),
      bind_group_layouts: &[Some(&bind_group_layout), Some(&shadow_bind_group_layout)],
      immediate_size: 0,
    });

    // The shadow pass only ever writes depth from `vs_shadow`, which reads
    // nothing but group 0 (for `light_view_proj`) -- a shorter layout than the
    // main world pipelines.
    let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("shadow-pl"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    let make_uniform_buffer = |label| {
      device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size_of::<WorldUniforms>() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      })
    };
    let make_bind_group = |label, buffer: &wgpu::Buffer| {
      device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
          binding: 0,
          resource: buffer.as_entire_binding(),
        }],
      })
    };

    let uniform_buffer = make_uniform_buffer("world-uniforms");
    let bind_group = make_bind_group("world-bg", &uniform_buffer);
    let uniform_buffer_noclip = make_uniform_buffer("world-uniforms-noclip");
    let bind_group_noclip = make_bind_group("world-bg-noclip", &uniform_buffer_noclip);

    let alpha_blend = wgpu::BlendState {
      color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
      },
      alpha: wgpu::BlendComponent::OVER,
    };

    let build = |fs_entry: &str,
                 topology: wgpu::PrimitiveTopology,
                 blend: Option<wgpu::BlendState>,
                 depth_write: bool,
                 cull_mode: Option<wgpu::Face>|
     -> wgpu::RenderPipeline {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("world-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
          module: &shader,
          entry_point: Some("vs_main"),
          compilation_options: wgpu::PipelineCompilationOptions::default(),
          buffers: &[Some(Vert::LAYOUT)],
        },
        primitive: wgpu::PrimitiveState {
          topology,
          front_face: wgpu::FrontFace::Cw,
          // `CullType` BACK/FRONT/NONE selected per draw via `CULL_MODES`.
          cull_mode,
          ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
          format: depth_format,
          depth_write_enabled: Some(depth_write),
          depth_compare: Some(wgpu::CompareFunction::Less),
          stencil: wgpu::StencilState::default(),
          bias: wgpu::DepthBiasState::default(),
        }),
        // MSAA — single-sample for now.
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
          module: &shader,
          entry_point: Some(fs_entry),
          compilation_options: wgpu::PipelineCompilationOptions::default(),
          targets: &[Some(wgpu::ColorTargetState {
            format: color_format,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
          })],
        }),
        multiview_mask: None,
        cache: None,
      })
    };

    let mesh_opaque = CULL_MODES.map(|c| {
      build(
        "fs_mesh",
        wgpu::PrimitiveTopology::TriangleList,
        None,
        true,
        c,
      )
    });
    let mesh_translucent = CULL_MODES.map(|c| {
      build(
        "fs_mesh",
        wgpu::PrimitiveTopology::TriangleList,
        Some(alpha_blend),
        false,
        c,
      )
    });
    let line_opaque = build(
      "fs_line",
      wgpu::PrimitiveTopology::LineList,
      None,
      true,
      None,
    );
    let line_translucent = build(
      "fs_line",
      wgpu::PrimitiveTopology::LineList,
      Some(alpha_blend),
      false,
      None,
    );

    let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("shadow-map"),
      size: wgpu::Extent3d {
        width: SHADOW_MAP_SIZE,
        height: SHADOW_MAP_SIZE,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: SHADOW_MAP_FORMAT,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());
    // `fs_mesh` already clamps the light-space UV/depth to `[0, 1]` before
    // sampling, so edge behaviour past the frustum never actually matters --
    // plain `ClampToEdge` avoids fussing with a border color.
    let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("shadow-sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      compare: Some(wgpu::CompareFunction::LessEqual),
      ..Default::default()
    });
    let shadow_tex_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("shadow-tex-bg"),
      layout: &shadow_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&shadow_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&shadow_sampler),
        },
      ],
    });

    let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("shadow-pipeline"),
      layout: Some(&shadow_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_shadow"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[Some(Vert::LAYOUT)],
      },
      // No back/front culling: the player model is a thin box/sphere, and
      // culling backfaces from the light's view can let light leak through
      // the near side onto the caster's own footprint (peter-panning).
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Cw,
        cull_mode: None,
        ..Default::default()
      },
      depth_stencil: Some(wgpu::DepthStencilState {
        format: SHADOW_MAP_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
      }),
      multisample: wgpu::MultisampleState::default(),
      fragment: None,
      multiview_mask: None,
      cache: None,
    });

    Self {
      uniform_buffer,
      bind_group,
      uniform_buffer_noclip,
      bind_group_noclip,
      mesh_opaque,
      line_opaque,
      mesh_translucent,
      line_translucent,
      shadow_pipeline,
      shadow_view,
      shadow_tex_bind_group,
    }
  }

  /// The shadow map's depth attachment -- render the player-only depth
  /// pre-pass into this before the main world pass each frame.
  pub fn shadow_view(&self) -> &wgpu::TextureView {
    &self.shadow_view
  }

  /// The depth-only `vs_shadow` pipeline for the shadow pre-pass.
  pub fn shadow_pipeline(&self) -> &wgpu::RenderPipeline {
    &self.shadow_pipeline
  }

  /// The per-frame `setMat4` / `setVec3` block. Uploads the uniforms twice: once
  /// as given (for [`Self::bind_group`]) and once with `player_pos.w = 0` (for
  /// [`Self::bind_group_noclip`], used for the player / ghost models).
  pub fn set_uniforms(&self, queue: &wgpu::Queue, u: &WorldUniforms) {
    let mut clip = *u;
    clip.player_pos.w = 1.0;
    queue.write_buffer(
      &self.uniform_buffer,
      0,
      as_bytes(std::slice::from_ref(&clip)),
    );

    let mut noclip = *u;
    noclip.player_pos.w = 0.0;
    queue.write_buffer(
      &self.uniform_buffer_noclip,
      0,
      as_bytes(std::slice::from_ref(&noclip)),
    );
  }

  /// The mesh pipeline for the given translucency + cull mode. `cull` must be one
  /// of [`CULL_MODES`]; an unrecognized value falls back to the no-cull variant.
  pub fn mesh(&self, translucent: bool, cull: Option<wgpu::Face>) -> &wgpu::RenderPipeline {
    let idx = CULL_MODES.iter().position(|c| *c == cull).unwrap_or(0);
    if translucent {
      &self.mesh_translucent[idx]
    } else {
      &self.mesh_opaque[idx]
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use glam::{Mat3, Vec3};
  use std::mem::offset_of;

  #[test]
  fn world_uniforms_size_and_offsets() {
    assert_eq!(size_of::<WorldUniforms>(), 416);
    assert_eq!(align_of::<WorldUniforms>(), 16);
    assert_eq!(offset_of!(WorldUniforms, model), 0);
    assert_eq!(offset_of!(WorldUniforms, view), 64);
    assert_eq!(offset_of!(WorldUniforms, projection), 128);
    assert_eq!(offset_of!(WorldUniforms, normal_matrix), 192);
    assert_eq!(offset_of!(WorldUniforms, view_pos), 256);
    assert_eq!(offset_of!(WorldUniforms, light_dir), 272);
    assert_eq!(offset_of!(WorldUniforms, player_pos), 288);
    assert_eq!(offset_of!(WorldUniforms, clip_params), 304);
    assert_eq!(offset_of!(WorldUniforms, clip_params2), 320);
    assert_eq!(offset_of!(WorldUniforms, light_view_proj), 336);
    assert_eq!(offset_of!(WorldUniforms, shadow_params), 400);
  }

  #[test]
  fn from_matrices_normal_matrix_is_inverse_transpose_of_model() {
    let model = Mat4::from_scale(glam::Vec3::new(2.0, 4.0, 8.0));
    let u = WorldUniforms::from_matrices(
      model,
      Mat4::IDENTITY,
      Mat4::IDENTITY,
      Vec3::ZERO,
      Vec3::Z,
      Vec3::ZERO,
    );
    let expected = Mat4::from_mat3(Mat3::from_mat4(model).inverse().transpose());
    assert_eq!(u.normal_matrix, expected);
    // inverse-transpose of a pure scale is 1/scale on the diagonal.
    assert!((u.normal_matrix.x_axis.x - 0.5).abs() < 1e-6);
    assert!((u.normal_matrix.y_axis.y - 0.25).abs() < 1e-6);
    assert!((u.normal_matrix.z_axis.z - 0.125).abs() < 1e-6);
  }

  #[test]
  fn world_shader_wgsl_parses() {
    // `wgpu::naga` is wgpu-core's re-export (native builds enable wgpu_core).
    wgpu::naga::front::wgsl::parse_str(WORLD_SHADER_WGSL)
      .expect("WORLD_SHADER_WGSL should parse cleanly");
  }
}
