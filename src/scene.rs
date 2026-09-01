//! P1.3 spike: render a rotating 3D cube into an offscreen wgpu texture so it can
//! be composited into the egui UI via `egui::Image` (see the "Chosen pattern B"
//! decision in `TASKS.md`).
//!
//! Reference only (NOT ported here): `../primewatch2/src/world/WorldRenderer.cpp`
//! `WorldRenderer::render` — the world pass owns its own projection / view / clear.
//! The real world renderer lands in Phase 8; this is a throwaway rotating primitive
//! that just proves the offscreen-target plumbing and exercises the depth buffer.

use std::time::Instant;

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

/// egui-wgpu hard-requires `Rgba8Unorm` for `register_native_texture`
/// (`egui-wgpu-0.36.1/src/renderer.rs:770`). Note for Phase 8: this is a linear
/// target, not the surface's sRGB format — colours written here are not
/// gamma-encoded, so the real renderer must account for that.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Cube corners: 3 floats position, 3 floats colour (colour derived from the
/// 0..1 remap of the corner so every face is visibly distinct).
#[rustfmt::skip]
const VERTICES: [f32; 8 * 6] = [
  -1.0, -1.0, -1.0,  0.0, 0.0, 0.0,
   1.0, -1.0, -1.0,  1.0, 0.0, 0.0,
   1.0,  1.0, -1.0,  1.0, 1.0, 0.0,
  -1.0,  1.0, -1.0,  0.0, 1.0, 0.0,
  -1.0, -1.0,  1.0,  0.0, 0.0, 1.0,
   1.0, -1.0,  1.0,  1.0, 0.0, 1.0,
   1.0,  1.0,  1.0,  1.0, 1.0, 1.0,
  -1.0,  1.0,  1.0,  0.0, 1.0, 1.0,
];

#[rustfmt::skip]
const INDICES: [u16; 36] = [
  0, 1, 2, 0, 2, 3, // -Z
  4, 6, 5, 4, 7, 6, // +Z
  0, 4, 5, 0, 5, 1, // -Y
  3, 2, 6, 3, 6, 7, // +Y
  0, 3, 7, 0, 7, 4, // -X
  1, 5, 6, 1, 6, 2, // +X
];

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
  wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

const SHADER_SRC: &str = r#"
struct Uniforms { mvp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
  @builtin(position) clip_pos: vec4<f32>,
  @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) color: vec3<f32>) -> VsOut {
  var out: VsOut;
  out.clip_pos = u.mvp * vec4<f32>(pos, 1.0);
  out.color = color;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  return vec4<f32>(in.color, 1.0);
}
"#;

/// Reinterpret a slice of plain-old-data (`f32` / `u16` here) as bytes for GPU
/// upload. `u8` has alignment 1 so any `[T]` is validly viewable as `[u8]`.
fn as_bytes<T: Copy>(data: &[T]) -> &[u8] {
  // SAFETY: `T: Copy` POD, read-only view, size in bytes is exact.
  unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(data)) }
}

fn clamp_size(size: (u32, u32)) -> (u32, u32) {
  (size.0.max(1), size.1.max(1))
}

pub struct SpikeScene {
  size: (u32, u32),
  color_view: wgpu::TextureView,
  depth_view: wgpu::TextureView,
  pipeline: wgpu::RenderPipeline,
  vertex_buf: wgpu::Buffer,
  index_buf: wgpu::Buffer,
  mvp_buf: wgpu::Buffer,
  bind_group: wgpu::BindGroup,
  start: Instant,
}

impl SpikeScene {
  pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
    let size = clamp_size(size);
    let (color_view, depth_view) = create_targets(device, size);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("spike-scene-shader"),
      source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("spike-scene-bgl"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("spike-scene-pl"),
      // wgpu 30: `&[Option<&BindGroupLayout>]`, and `immediate_size` replaces
      // `push_constant_ranges`.
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("spike-scene-pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[Some(wgpu::VertexBufferLayout {
          array_stride: (6 * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
          step_mode: wgpu::VertexStepMode::Vertex,
          attributes: &VERTEX_ATTRS,
        })],
      },
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        cull_mode: Some(wgpu::Face::Back),
        ..Default::default()
      },
      depth_stencil: Some(wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        // wgpu 30: `Option<bool>` / `Option<CompareFunction>` (was bare values in <=24).
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
      }),
      // TODO(P8): MSAA — the spike renders single-sampled.
      multisample: wgpu::MultisampleState::default(),
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        targets: &[Some(wgpu::ColorTargetState {
          format: COLOR_FORMAT,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
      }),
      multiview_mask: None,
      cache: None,
    });

    let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("spike-scene-vertices"),
      contents: as_bytes(&VERTICES),
      usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("spike-scene-indices"),
      contents: as_bytes(&INDICES),
      usage: wgpu::BufferUsages::INDEX,
    });
    let mvp_buf = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("spike-scene-mvp"),
      size: 64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("spike-scene-bg"),
      layout: &bind_group_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: mvp_buf.as_entire_binding(),
      }],
    });

    Self {
      size,
      color_view,
      depth_view,
      pipeline,
      vertex_buf,
      index_buf,
      mvp_buf,
      bind_group,
      start: Instant::now(),
    }
  }

  /// The offscreen colour target — handed to egui as a user texture.
  pub fn color_view(&self) -> &wgpu::TextureView {
    &self.color_view
  }

  /// Recreate the colour + depth targets if `size` changed. Returns `true` when
  /// the targets were recreated, so the caller re-registers the egui texture.
  pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) -> bool {
    let size = clamp_size(size);
    if size == self.size {
      return false;
    }
    let (color_view, depth_view) = create_targets(device, size);
    self.color_view = color_view;
    self.depth_view = depth_view;
    self.size = size;
    true
  }

  /// Render one frame of the rotating cube into the offscreen target. Adds a
  /// dedicated render pass to `encoder`; the caller submits it alongside the
  /// egui swapchain pass.
  pub fn render(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
    let t = self.start.elapsed().as_secs_f32();
    let (w, h) = self.size;
    let aspect = w as f32 / h as f32;

    // RH world space, DirectX-style [0, 1] clip depth (wgpu convention).
    let proj =
      glam::camera::rh::proj::directx::perspective(60.0_f32.to_radians(), aspect, 0.1, 100.0);
    let view = glam::camera::rh::view::look_at_mat4(Vec3::new(2.5, 2.0, 4.0), Vec3::ZERO, Vec3::Y);
    let model = Mat4::from_rotation_y(t) * Mat4::from_rotation_x(t * 0.6);
    let mvp = (proj * view * model).to_cols_array();
    queue.write_buffer(&self.mvp_buf, 0, as_bytes(&mvp));

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("spike-scene-pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: &self.color_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color {
            r: 0.05,
            g: 0.05,
            b: 0.12,
            a: 1.0,
          }),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
        view: &self.depth_view,
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

    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, &self.bind_group, &[]);
    pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
    pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint16);
    pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1);
  }
}

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
    label: Some("spike-scene-color"),
    size: extent,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: COLOR_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let depth = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("spike-scene-depth"),
    size: extent,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: DEPTH_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    view_formats: &[],
  });
  (
    color.create_view(&wgpu::TextureViewDescriptor::default()),
    depth.create_view(&wgpu::TextureViewDescriptor::default()),
  )
}
