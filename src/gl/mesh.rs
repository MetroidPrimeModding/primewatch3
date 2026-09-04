//! `DynamicMesh` — a growable vertex buffer plus a non-indexed draw.

use crate::gl::{Vert, as_bytes};

/// Initial buffer capacity
const INITIAL_CAPACITY_BYTES: u64 = 4096;

pub struct DynamicMesh {
  label: String,
  buffer: wgpu::Buffer,
  capacity_bytes: u64,
  vert_count: u32,
}

impl DynamicMesh {
  pub fn new(device: &wgpu::Device, label: &str) -> Self {
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
    }
  }

  /// `bufferNewData`: grow the buffer if needed, then upload.
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

  /// The pipeline + bind group are bound by the caller, once before the draw loop.
  pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
    if self.vert_count == 0 {
      return;
    }
    pass.set_vertex_buffer(0, self.buffer.slice(..));
    pass.draw(0..self.vert_count, 0..1);
  }
}
