//! A small reimplementation of Dear ImGui's `ImGuiTextFilter` — the widget the
//! C++ "Objects" window used to filter the entity table (`objectFilter` in
//! `drawObjectsWindow`).
//!
//! Semantics ported from `imgui.cpp` `ImGuiTextFilter::Build` / `PassFilter`:
//!
//! * The raw string is split on `,`; each term is trimmed and empty terms are
//!   dropped.
//! * A term with a leading `-` is a *negative* filter — if its remainder is a
//!   substring of the tested text the text is rejected.
//! * A term without `-` is a *positive* (grep) filter.
//! * An empty filter (no terms) passes everything.
//! * Otherwise: any negative match rejects; then, if there are positive terms
//!   and none matched, reject; else pass.
//!
//! Deviations from stock ImGui, both deliberate:
//!
//! * Matching is **case-sensitive** substring (ImGui's `ImStristr` is
//!   case-insensitive) — the filter probe strings are built with fixed casing.
//! * Negative terms are always evaluated before a positive match can pass.
//!   Stock ImGui iterates the term list in order and returns `true` on the
//!   first positive hit, so `"foo,-bar"` there passes `"foo bar"`; here the
//!   `-bar` still rejects it. This is the order-independent reading the port
//!   targets.

/// The `ImGuiTextFilter` analogue: just the raw, comma-separated filter text.
#[derive(Debug, Default, Clone)]
pub struct ObjectFilter {
  /// The unparsed filter text, as typed into the text box.
  pub raw: String,
}

impl ObjectFilter {
  /// Mirrors `ImGuiTextFilter::PassFilter`. Returns `true` if `text` should be
  /// shown given the current filter.
  pub fn passes(&self, text: &str) -> bool {
    let terms: Vec<&str> = self
      .raw
      .split(',')
      .map(str::trim)
      .filter(|t| !t.is_empty())
      .collect();
    if terms.is_empty() {
      return true;
    }

    let mut positive_terms = 0u32;
    let mut positive_hit = false;
    for term in terms {
      if let Some(rest) = term.strip_prefix('-') {
        // Negative filter — a bare "-" has no remainder and is a no-op.
        if !rest.is_empty() && text.contains(rest) {
          return false;
        }
      } else {
        positive_terms += 1;
        if text.contains(term) {
          positive_hit = true;
        }
      }
    }

    // Pass if a positive term matched, or if there were none to match
    // (ImGui's implicit "* grep" when `CountGrep == 0`).
    positive_terms == 0 || positive_hit
  }

  /// egui helper: draw the filter text box (C++ `objectFilter.Draw()`, whose
  /// default label is `"Filter (inc,-exc)"`).
  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
      ui.label("Filter (inc,-exc)");
      ui.text_edit_singleline(&mut self.raw);
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn f(raw: &str) -> ObjectFilter {
    ObjectFilter {
      raw: raw.to_string(),
    }
  }

  #[test]
  fn empty_filter_passes_everything() {
    let filter = f("");
    assert!(filter.passes("anything"));
    assert!(filter.passes(""));
  }

  #[test]
  fn positive_term_is_an_include_substring() {
    let filter = f("foo");
    assert!(filter.passes("a foo bar"));
    assert!(filter.passes("foo"));
    assert!(!filter.passes("bar"));
  }

  #[test]
  fn negative_term_is_an_exclude_substring() {
    let filter = f("-foo");
    assert!(!filter.passes("a foo bar"));
    assert!(filter.passes("bar"));
  }

  #[test]
  fn combo_include_and_exclude() {
    let filter = f("foo,-bar");
    assert!(filter.passes("foo baz"));
    assert!(!filter.passes("foo bar")); // excluded wins
    assert!(!filter.passes("baz")); // no positive match
  }

  #[test]
  fn only_negative_terms_pass_when_not_excluded() {
    let filter = f("-bar,-baz");
    assert!(filter.passes("foo"));
    assert!(!filter.passes("foo bar"));
  }

  #[test]
  fn whitespace_around_terms_is_trimmed() {
    let filter = f("  foo ,  -bar  ");
    assert!(filter.passes("foo"));
    assert!(!filter.passes("foo bar"));
    assert!(!filter.passes("  ")); // empty terms dropped, "foo" still required
  }

  #[test]
  fn matching_is_case_sensitive() {
    let filter = f("Foo");
    assert!(filter.passes("Foo"));
    assert!(!filter.passes("foo"));
  }
}
