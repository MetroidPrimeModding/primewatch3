//! The read-only "Raw view" hex dump window over the MEM1 snapshot.

/// A minimal read-only hex dump over the MEM1 snapshot — a small custom table
/// rather than adding `egui_memory_editor`. Offsets are raw snapshot offsets (base 0)
pub(super) fn render_raw_data_view(ui: &mut egui::Ui, data: &[u8]) {
  const BYTES_PER_ROW: usize = 16;
  let rows = data.len().div_ceil(BYTES_PER_ROW);
  let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
  egui::ScrollArea::vertical()
    .auto_shrink([false, false])
    .show_rows(ui, row_h, rows, |ui, range| {
      for row in range {
        let start = row * BYTES_PER_ROW;
        let end = (start + BYTES_PER_ROW).min(data.len());
        let chunk = &data[start..end];
        let mut line = format!("{start:08x}  ");
        for b in chunk {
          line.push_str(&format!("{b:02x} "));
        }
        for _ in chunk.len()..BYTES_PER_ROW {
          line.push_str("   ");
        }
        line.push(' ');
        for &b in chunk {
          line.push(if (0x20..0x7f).contains(&b) {
            b as char
          } else {
            '.'
          });
        }
        ui.add(
          egui::Label::new(egui::RichText::new(line).monospace())
            .wrap_mode(egui::TextWrapMode::Extend),
        );
      }
    });
}
