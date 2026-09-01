//! `WorldUniforms` + WGSL + `WorldPipelines` — ports
//! `../primewatch2/src/gl/OpenGLShader.{hpp,cpp}` and the three GLSL shader
//! strings in `WorldRenderer.cpp:31-113` (plus the per-frame uniform poke block
//! at `WorldRenderer.cpp:338-406`).

use crate::gl::{Vert, as_bytes};

/// Uniforms poked by `meshShader->setMat4` / `setVec3` in `WorldRenderer::render`
/// (`WorldRenderer.cpp:338-350`): `model`, `view`, `projection`, `viewPos`,
/// `lightDir`.
///
/// The C++ vertex shader computes the normal matrix in-shader via
/// `transpose(inverse(model))`. WGSL has no `inverse()`, so it moves to a
/// CPU-supplied `normal_matrix` uniform (sanctioned deviation, standard
/// practice).
///
/// C++ passes `lightDir` already normalized
/// (`setVec3("lightDir", glm::normalize(lightDir))`) — that stays the caller's
/// (P8.4) job.
///
/// `size_of::<WorldUniforms>() == 288`, align 16; field offsets match the WGSL
/// `Uniforms` struct layout (see the module test).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WorldUniforms {
  pub model: [[f32; 4]; 4],
  pub view: [[f32; 4]; 4],
  pub projection: [[f32; 4]; 4],
  /// `Mat4::from_mat3(Mat3::from_mat4(model).inverse().transpose())`.
  pub normal_matrix: [[f32; 4]; 4],
  pub view_pos: [f32; 3],
  pub _pad0: f32,
  pub light_dir: [f32; 3],
  pub _pad1: f32,
}

impl Default for WorldUniforms {
  fn default() -> Self {
    let ident = glam::Mat4::IDENTITY.to_cols_array_2d();
    Self {
      model: ident,
      view: ident,
      projection: ident,
      normal_matrix: ident,
      view_pos: [0.0; 3],
      _pad0: 0.0,
      light_dir: [0.0; 3],
      _pad1: 0.0,
    }
  }
}

impl WorldUniforms {
  /// Build the uniform block from the matrices the caller already has, filling
  /// `normal_matrix` from `model`. `light_dir` is taken as-is (caller normalizes,
  /// per the C++ `glm::normalize(lightDir)`).
  pub fn from_matrices(
    model: glam::Mat4,
    view: glam::Mat4,
    projection: glam::Mat4,
    view_pos: glam::Vec3,
    light_dir: glam::Vec3,
  ) -> Self {
    let normal_matrix = glam::Mat4::from_mat3(glam::Mat3::from_mat4(model).inverse().transpose());
    Self {
      model: model.to_cols_array_2d(),
      view: view.to_cols_array_2d(),
      projection: projection.to_cols_array_2d(),
      normal_matrix: normal_matrix.to_cols_array_2d(),
      view_pos: view_pos.to_array(),
      _pad0: 0.0,
      light_dir: light_dir.to_array(),
      _pad1: 0.0,
    }
  }
}

/// One WGSL module: `vs_main` (ports `meshVertShader`, shared), `fs_mesh` (ports
/// `meshFragShader`) and `fs_line` (ports `lineFragShader`).
///
/// **Gamma (flag for reviewer):** the `linear_to_srgb` on every final output is
/// *new* — the C++ wrote raw values to a plain GL backbuffer. It implements the
/// P1.3 contract ("the real renderer must do its own linear→sRGB") because egui
/// composites the linear `Rgba8Unorm` target without re-encoding. If P8.4
/// compositing shows double-encoding, revisit here.
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

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
  let lo = c * 12.92;
  let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c < vec3<f32>(0.0031308));
}

@fragment
fn fs_mesh(in: VsOut) -> @location(0) vec4<f32> {
  let edge_thickness = 0.015;
  let min_bary = min(min(in.barycentric.x, in.barycentric.y), in.barycentric.z);
  if (min_bary > 0.0 && min_bary < edge_thickness) {
    return vec4<f32>(linear_to_srgb(vec3<f32>(0.2)), 1.0);
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
  return vec4<f32>(linear_to_srgb(lit.rgb), lit.a);
}

@fragment
fn fs_line(in: VsOut) -> @location(0) vec4<f32> {
  let out = vec4<f32>(1.0, 1.0, 1.0, 1.0) * in.color;
  return vec4<f32>(linear_to_srgb(out.rgb), out.a);
}
"#;

/// The mesh + line pipelines (opaque + translucent variants) plus the shared
/// uniform buffer / bind group — ports `OpenGLShader`'s compile+link
/// (`OpenGLShader.cpp:8-45`) and the GL state set in
/// `WorldRenderer.cpp:352-403`.
pub struct WorldPipelines {
  pub bind_group_layout: wgpu::BindGroupLayout,
  pub uniform_buffer: wgpu::Buffer,
  pub bind_group: wgpu::BindGroup,
  pub mesh_opaque: wgpu::RenderPipeline,
  pub line_opaque: wgpu::RenderPipeline,
  pub mesh_translucent: wgpu::RenderPipeline,
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
      size: std::mem::size_of::<WorldUniforms>() as wgpu::BufferAddress,
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

    // ports `glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)`
    // (`WorldRenderer.cpp:394-395`).
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
                 depth_write: bool|
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
          // ports `glFrontFace(GL_CW)` (`WorldRenderer.cpp:355`).
          front_face: wgpu::FrontFace::Cw,
          // P8.4: owns the `CullType` BACK/FRONT/NONE → pipeline-variant choice.
          cull_mode: None,
          ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
          format: depth_format,
          // ports `glEnable(GL_DEPTH_TEST)` + `glDepthFunc(GL_LESS)` + the
          // translucent `glDepthMask(GL_FALSE)` (`WorldRenderer.cpp:352-403`).
          depth_write_enabled: Some(depth_write),
          depth_compare: Some(wgpu::CompareFunction::Less),
          stencil: wgpu::StencilState::default(),
          bias: wgpu::DepthBiasState::default(),
        }),
        // TODO(P8.4): MSAA — single-sample for now (matches `scene.rs`).
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

    let mesh_opaque = build("fs_mesh", wgpu::PrimitiveTopology::TriangleList, None, true);
    let line_opaque = build("fs_line", wgpu::PrimitiveTopology::LineList, None, true);
    let mesh_translucent = build(
      "fs_mesh",
      wgpu::PrimitiveTopology::TriangleList,
      Some(alpha_blend),
      false,
    );
    let line_translucent = build(
      "fs_line",
      wgpu::PrimitiveTopology::LineList,
      Some(alpha_blend),
      false,
    );

    Self {
      bind_group_layout,
      uniform_buffer,
      bind_group,
      mesh_opaque,
      line_opaque,
      mesh_translucent,
      line_translucent,
    }
  }

  /// Ports the per-frame `setMat4` / `setVec3` block
  /// (`WorldRenderer.cpp:338-350`).
  pub fn set_uniforms(&self, queue: &wgpu::Queue, u: &WorldUniforms) {
    queue.write_buffer(&self.uniform_buffer, 0, as_bytes(std::slice::from_ref(u)));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn world_uniforms_size_and_offsets() {
    assert_eq!(std::mem::size_of::<WorldUniforms>(), 288);
    assert_eq!(std::mem::align_of::<WorldUniforms>(), 4);
    assert_eq!(std::mem::offset_of!(WorldUniforms, model), 0);
    assert_eq!(std::mem::offset_of!(WorldUniforms, view), 64);
    assert_eq!(std::mem::offset_of!(WorldUniforms, projection), 128);
    assert_eq!(std::mem::offset_of!(WorldUniforms, normal_matrix), 192);
    assert_eq!(std::mem::offset_of!(WorldUniforms, view_pos), 256);
    assert_eq!(std::mem::offset_of!(WorldUniforms, light_dir), 272);
  }

  #[test]
  fn from_matrices_normal_matrix_is_inverse_transpose_of_model() {
    let model = glam::Mat4::from_scale(glam::Vec3::new(2.0, 4.0, 8.0));
    let u = WorldUniforms::from_matrices(
      model,
      glam::Mat4::IDENTITY,
      glam::Mat4::IDENTITY,
      glam::Vec3::ZERO,
      glam::Vec3::Z,
    );
    let expected =
      glam::Mat4::from_mat3(glam::Mat3::from_mat4(model).inverse().transpose()).to_cols_array_2d();
    assert_eq!(u.normal_matrix, expected);
    // inverse-transpose of a pure scale is 1/scale on the diagonal.
    assert!((u.normal_matrix[0][0] - 0.5).abs() < 1e-6);
    assert!((u.normal_matrix[1][1] - 0.25).abs() < 1e-6);
    assert!((u.normal_matrix[2][2] - 0.125).abs() < 1e-6);
  }

  #[test]
  fn world_shader_wgsl_parses() {
    // `wgpu::naga` is wgpu-core's re-export (native builds enable wgpu_core).
    wgpu::naga::front::wgsl::parse_str(WORLD_SHADER_WGSL)
      .expect("WORLD_SHADER_WGSL should parse cleanly");
  }
}
