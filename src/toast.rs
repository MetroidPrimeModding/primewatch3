//! Minimal ephemeral toast notifications for egui 0.36.
//!
//! `egui-notify` has no release compatible with egui 0.36 yet (0.22 caps at
//! egui `^0.34`), and the need here is small: a corner stack of self-dismissing
//! messages to replace the permanent "Prime Watch" status window.

use std::time::{Duration, Instant};

/// How long an info toast stays fully visible before it starts fading.
const INFO_TTL: Duration = Duration::from_secs(4);
/// Errors linger — they usually matter and the user may have looked away.
const ERROR_TTL: Duration = Duration::from_secs(12);
/// Fade-in / fade-out ramp length.
const FADE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
  Info,
  Error,
}

struct Toast {
  text: String,
  kind: ToastKind,
  ttl: Duration,
  /// Set on the toast's first painted frame, so its clock starts when the user
  /// can actually see it (the window can appear a beat after `Toasts::info` is
  /// called at startup).
  shown_at: Option<Instant>,
}

/// A stack of pending toasts. Owned by `App`, drawn once per frame via [`ui`].
///
/// [`ui`]: Toasts::ui
#[derive(Default)]
pub struct Toasts {
  items: Vec<Toast>,
}

impl Toasts {
  pub fn info(&mut self, text: impl Into<String>) {
    self.items.push(Toast {
      text: text.into(),
      kind: ToastKind::Info,
      ttl: INFO_TTL,
      shown_at: None,
    });
  }

  pub fn error(&mut self, text: impl Into<String>) {
    self.items.push(Toast {
      text: text.into(),
      kind: ToastKind::Error,
      ttl: ERROR_TTL,
      shown_at: None,
    });
  }

  /// Paint the stack in the top-right corner, drop expired toasts, and dismiss
  /// any that were clicked. Call once per frame, after the rest of the UI so the
  /// toasts float above it.
  pub fn ui(&mut self, ctx: &egui::Context) {
    if self.items.is_empty() {
      return;
    }

    let now = Instant::now();
    let mut clicked: Option<usize> = None;
    egui::Area::new(egui::Id::new("toasts"))
      .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
      .order(egui::Order::Foreground)
      .interactable(true)
      .show(ctx, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
          ui.spacing_mut().item_spacing.y = 6.0;
          for (i, t) in self.items.iter_mut().enumerate() {
            let shown_at = *t.shown_at.get_or_insert(now);
            let age = now.saturating_duration_since(shown_at);
            let remaining = t.ttl.saturating_sub(age);
            let alpha = if age < FADE {
              age.as_secs_f32() / FADE.as_secs_f32()
            } else if remaining < FADE {
              remaining.as_secs_f32() / FADE.as_secs_f32()
            } else {
              1.0
            }
            .clamp(0.0, 1.0);

            let (bg, fg) = match t.kind {
              ToastKind::Info => {
                (ui.visuals().window_fill(), ui.visuals().text_color())
              }
              ToastKind::Error => {
                (egui::Color32::from_rgb(122, 32, 32), egui::Color32::WHITE)
              }
            };

            let resp = egui::Frame::NONE
              .fill(bg.gamma_multiply(alpha))
              .inner_margin(8.0)
              .corner_radius(4.0)
              .show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.label(
                  egui::RichText::new(&t.text).color(fg.gamma_multiply(alpha)),
                );
              })
              .response
              .interact(egui::Sense::click());
            if resp.clicked() {
              clicked = Some(i);
            }
          }
        });
      });

    if let Some(i) = clicked {
      self.items.remove(i);
    }
    self
      .items
      .retain(|t| t.shown_at.is_none_or(|s| now.saturating_duration_since(s) < t.ttl));

    // Keep the fade animating and expiry prompt even with no other input.
    ctx.request_repaint();
  }
}
