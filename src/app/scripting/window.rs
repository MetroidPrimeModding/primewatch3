use crate::structs::prime_structs::GameInstance;

#[derive(Clone)]
pub enum CustomInspectorRow {
  /// A live handle: re-resolved and re-read from memory by the inspector every
  /// frame. `label` is the tree-node caption.
  Instance {
    label: String,
    instance: GameInstance,
  },
  Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorAlign {
  Min,
  Center,
  Max,
}

#[derive(Clone, Copy)]
pub struct WindowAnchor {
  pub align: (AnchorAlign, AnchorAlign),
  pub offset: (f32, f32),
}

pub(super) fn parse_anchor(name: &str) -> Result<(AnchorAlign, AnchorAlign), String> {
  use AnchorAlign::*;
  match name {
    "left_top" => Ok((Min, Min)),
    "left_center" => Ok((Min, Center)),
    "left_bottom" => Ok((Min, Max)),
    "center_top" => Ok((Center, Min)),
    "center_center" => Ok((Center, Center)),
    "center_bottom" => Ok((Center, Max)),
    "right_top" => Ok((Max, Min)),
    "right_center" => Ok((Max, Center)),
    "right_bottom" => Ok((Max, Max)),
    other => Err(format!(
      "unknown anchor \"{other}\" (expected e.g. \"left_bottom\", \"center_center\", \"right_top\")"
    )),
  }
}

#[derive(Clone)]
pub struct CustomInspectorWindow {
  pub title: String,
  pub title_bar: bool,
  pub anchor: Option<WindowAnchor>,
  pub rows: Vec<CustomInspectorRow>,
}

impl CustomInspectorWindow {
  pub(super) fn new(title: String) -> Self {
    CustomInspectorWindow {
      title,
      title_bar: true,
      anchor: None,
      rows: Vec::new(),
    }
  }

  pub(super) fn add_instance(&mut self, label: String, instance: GameInstance) {
    self
      .rows
      .push(CustomInspectorRow::Instance { label, instance });
  }

  pub(super) fn add_text(&mut self, text: String) {
    self.rows.push(CustomInspectorRow::Text(text));
  }
}
