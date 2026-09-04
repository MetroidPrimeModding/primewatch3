//! `WorldUniforms` + WGSL + `WorldPipelines` — the uniform block, the world
//! shader, and the render pipelines (opaque + translucent, mesh + line), plus
//! the per-frame uniform upload.

use glam::{Mat4, Vec3A};
use crate::gl::{Vert, as_bytes};

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
  pub player_pos: Vec3A,
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
      player_pos: Vec3A::ZERO
    }
  }
}

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
      player_pos: player_pos.into(),
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
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
  @builtin(position) clip_pos: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) frag_pos: vec3<f32>,
  @location(3) barycentric: vec3<f32>,
};

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

@fragment
fn fs_mesh(in: VsOut) -> @location(0) vec4<f32> {
  let edge_thickness = 0.015;
  let min_bary = min(min(in.barycentric.x, in.barycentric.y), in.barycentric.z);
  if (min_bary > 0.0 && min_bary < edge_thickness) {
    return vec4<f32>(vec3<f32>(0.2), 1.0);
  }
  let light_color = vec3<f32>(1.0, 1.0, 1.0);
  let ambient = 0.7 * light_color;
  let diff = max(dot(in.normal, u.light_dir), 0.0);
  let diffuse = diff * light_color * 0.5;
  let view_dir = normalize(u.view_pos - in.frag_pos);
  let reflect_dir = reflect(-u.light_dir, in.normal);
  let spec = pow(max(dot(view_dir, reflect_dir), 0.0), 256.0);
  let specular = 0.2 * spec * light_color;
  let lit = vec4<f32>(ambient + diffuse + specular, 1.0) * in.color;
  return lit;
}

@fragment
fn fs_line(in: VsOut) -> @location(0) vec4<f32> {
  return vec4<f32>(1.0, 1.0, 1.0, 1.0) * in.color;
}
"#;

/// Cull-mode order for the mesh-pipeline arrays: `[0]` = no culling, `[1]` =
/// back-face, `[2]` = front-face. Mirrors the `switch (culling)` in
/// `WorldRenderer` — a variant is selected per `CullType` at draw
/// (`front_face: Cw` is baked; lines are never culled).
pub const CULL_MODES: [Option<wgpu::Face>; 3] =
  [None, Some(wgpu::Face::Back), Some(wgpu::Face::Front)];

/// The mesh + line pipelines (opaque + translucent variants) plus the shared
/// uniform buffer / bind group
///
/// The two mesh pipelines exist in all three [`CULL_MODES`] variants
/// pick one with [`WorldPipelines::mesh`].
pub struct WorldPipelines {
  pub uniform_buffer: wgpu::Buffer,
  pub bind_group: wgpu::BindGroup,
  mesh_opaque: [wgpu::RenderPipeline; 3],
  mesh_translucent: [wgpu::RenderPipeline; 3],
  pub line_opaque: wgpu::RenderPipeline,
  pub line_translucent: wgpu::RenderPipeline,
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

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("world-pl"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("world-uniforms"),
      size: size_of::<WorldUniforms>() as wgpu::BufferAddress,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("world-bg"),
      layout: &bind_group_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: uniform_buffer.as_entire_binding(),
      }],
    });

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

    Self {
      uniform_buffer,
      bind_group,
      mesh_opaque,
      line_opaque,
      mesh_translucent,
      line_translucent,
    }
  }

  /// The per-frame `setMat4` / `setVec3` block.
  pub fn set_uniforms(&self, queue: &wgpu::Queue, u: &WorldUniforms) {
    queue.write_buffer(&self.uniform_buffer, 0, as_bytes(std::slice::from_ref(u)));
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
  use std::mem::offset_of;
  use glam::{Mat3, Vec3};
  use super::*;

  #[test]
  fn world_uniforms_size_and_offsets() {
    assert_eq!(size_of::<WorldUniforms>(), 304);
    assert_eq!(align_of::<WorldUniforms>(), 16);
    assert_eq!(offset_of!(WorldUniforms, model), 0);
    assert_eq!(offset_of!(WorldUniforms, view), 64);
    assert_eq!(offset_of!(WorldUniforms, projection), 128);
    assert_eq!(offset_of!(WorldUniforms, normal_matrix), 192);
    assert_eq!(offset_of!(WorldUniforms, view_pos), 256);
    assert_eq!(offset_of!(WorldUniforms, light_dir), 272);
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
      Vec3::ZERO
    );
    let expected = Mat4::from_mat3(
        Mat3::from_mat4(model).inverse().transpose()
    );
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
