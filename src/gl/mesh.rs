//! `DynamicMesh` — ports `../primewatch2/src/gl/OpenGLMesh.{hpp,cpp}`.
//!
//! The GL `OpenGLMesh` owned a VAO + VBO and did the attrib setup itself; in
//! wgpu the vertex layout is pipeline state ([`Vert::LAYOUT`]), so this is just a
//! growable `VERTEX | COPY_DST` buffer plus a non-indexed draw (`glDrawArrays`).

use crate::gl::{Topology, Vert, as_bytes};

/// Initial buffer capacity — matches the "start empty, grow on demand" shape of
/// the C++ ctor (`OpenGLMesh.cpp:5-24`).
const INITIAL_CAPACITY_BYTES: u64 = 4096;

pub struct DynamicMesh {
  label: String,
  buffer: wgpu::Buffer,
  capacity_bytes: u64,
  vert_count: u32,
  topology: Topology,
}

impl DynamicMesh {
  /// Ports the ctor (`OpenGLMesh.cpp:5-24`), minus the VAO / attrib setup (now
  /// pipeline state): create an empty `VERTEX | COPY_DST` buffer.
  pub fn new(device: &wgpu::Device, label: &str, topology: Topology) -> Self {
    let capacity_bytes = INITIAL_CAPACITY_BYTES;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some(label),
      size: capacity_bytes,
      usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    Self {
      label: label.to_string(),
      buffer,
      capacity_bytes,
      vert_count: 0,
      topology,
    }
  }

  /// Ports `bufferNewData` (`OpenGLMesh.cpp:35-46`): grow the buffer if needed,
  /// then upload. The GL `STATIC` / `DYNAMIC` / `STREAM` hint has no wgpu
  /// analogue — dropped.
  pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, verts: &[Vert]) {
    let needed = std::mem::size_of_val(verts) as u64;
    if needed > self.capacity_bytes {
      self.capacity_bytes = needed.next_power_of_two().max(4);
      self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&self.label),
        size: self.capacity_bytes,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      });
    }
    if needed > 0 {
      queue.write_buffer(&self.buffer, 0, as_bytes(verts));
    }
    self.vert_count = verts.len() as u32;
  }

  /// Ports `draw()` (`OpenGLMesh.cpp:48-76`): non-indexed draw = `glDrawArrays`.
  /// The pipeline + bind group are bound by the caller (P8.4), matching the C++
  /// `meshShader->use()` once before the draw loop.
  pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
    if self.vert_count == 0 {
      return;
    }
    pass.set_vertex_buffer(0, self.buffer.slice(..));
    pass.draw(0..self.vert_count, 0..1);
  }

  pub fn vert_count(&self) -> u32 {
    self.vert_count
  }

  /// Informational — the actual topology lives in the pipeline the caller binds.
  pub fn topology(&self) -> Topology {
    self.topology
  }
}
