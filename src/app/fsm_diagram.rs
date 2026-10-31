//! Node-and-arrow drawing of a `CPatterned`'s state machine (`FsmGraph`).
//!
//! Layout is a BFS-layered grid rooted at state 0 (`Start`), computed from graph
//! topology alone; per-node drag offsets are layered on top and remembered per
//! `CStateMachine` address. Everything else (current-state highlight, edges) is
//! redrawn from the live graph each frame.

use std::collections::VecDeque;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::mem::ai_fsm::FsmGraph;

use super::state_history::HISTORY_LEN;

/// Highlight for the previous state, then the one before it.
pub(super) const RECENT_COLORS: [Color32; HISTORY_LEN] = [
  Color32::from_rgb(255, 214, 10),
  Color32::from_rgb(176, 150, 40),
];
const RECENT_FILLS: [Color32; HISTORY_LEN] = [
  Color32::from_rgb(104, 88, 12),
  Color32::from_rgb(70, 62, 24),
];
const NODE_PAD: Vec2 = Vec2::new(10.0, 6.0);
const COL_GAP: f32 = 110.0;
const ROW_GAP: f32 = 38.0;
const MARGIN: f32 = 30.0;
const EDGE_HIT_RADIUS: f32 = 6.0;
const BEZIER_SAMPLES: usize = 24;

/// All transitions from one state to another, collapsed into a single arrow.
#[derive(Debug, PartialEq)]
pub(super) struct Edge {
  pub from: usize,
  pub to: usize,
  /// Indices into `graph.states[from].chains`.
  pub chains: Vec<usize>,
}

pub(super) fn edges(graph: &FsmGraph) -> Vec<Edge> {
  let mut out: Vec<Edge> = Vec::new();
  for (from, state) in graph.states.iter().enumerate() {
    for (ci, chain) in state.chains.iter().enumerate() {
      let Some(to) = chain.target else { continue };
      match out.iter_mut().find(|e| e.from == from && e.to == to) {
        Some(e) => e.chains.push(ci),
        None => out.push(Edge {
          from,
          to,
          chains: vec![ci],
        }),
      }
    }
  }
  out
}

/// Tallest a column may get before it wraps into a second column to its right.
const MAX_COLUMN_ROWS: usize = 10;

/// `(column, row)` per state.
///
/// States entered from C++ rather than a trigger (`Dead`, `GetUpNow`, ...) have no
/// way in from `Start`, so every state without an incoming edge is a root too,
/// and a cycle nothing else reaches gets its lowest-index state as a root. Column
/// is distance from the nearest root; rows follow the average row of a state's
/// predecessors to keep arrows short. A column taller than `MAX_COLUMN_ROWS`
/// wraps into extra columns.
pub(super) fn grid_layout(graph: &FsmGraph) -> Vec<(usize, usize)> {
  let n = graph.states.len();
  let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
  let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
  for (from, state) in graph.states.iter().enumerate() {
    for t in state
      .chains
      .iter()
      .filter_map(|c| c.target)
      .filter(|&t| t < n)
    {
      succ[from].push(t);
      if t != from {
        pred[t].push(from);
      }
    }
  }

  let mut depth: Vec<Option<usize>> = vec![None; n];
  let mut queue = VecDeque::new();
  let seed = |s: usize, depth: &mut Vec<Option<usize>>, queue: &mut VecDeque<usize>| {
    if depth[s].is_none() {
      depth[s] = Some(0);
      queue.push_back(s);
    }
  };
  let flood = |depth: &mut Vec<Option<usize>>, queue: &mut VecDeque<usize>| {
    while let Some(s) = queue.pop_front() {
      let d = depth[s].unwrap_or(0);
      for &t in &succ[s] {
        if depth[t].is_none() {
          depth[t] = Some(d + 1);
          queue.push_back(t);
        }
      }
    }
  };
  if n > 0 {
    seed(0, &mut depth, &mut queue);
  }
  for s in (0..n).filter(|&s| pred[s].is_empty()) {
    seed(s, &mut depth, &mut queue);
  }
  flood(&mut depth, &mut queue);
  while let Some(s) = depth.iter().position(Option::is_none) {
    seed(s, &mut depth, &mut queue);
    flood(&mut depth, &mut queue);
  }
  let depth: Vec<usize> = depth.into_iter().map(|d| d.unwrap_or(0)).collect();

  let max_depth = depth.iter().copied().max().unwrap_or(0);
  let mut cells = vec![(0usize, 0usize); n];
  let mut row_of = vec![0.0f32; n];
  let mut next_col = 0;
  for d in 0..=max_depth {
    let mut members: Vec<usize> = (0..n).filter(|&s| depth[s] == d).collect();
    let key = |s: usize| {
      let rows: Vec<f32> = pred[s]
        .iter()
        .filter(|&&p| depth[p] < d)
        .map(|&p| row_of[p])
        .collect();
      if rows.is_empty() {
        f32::MAX
      } else {
        rows.iter().sum::<f32>() / rows.len() as f32
      }
    };
    // Stable sort keeps index order for ties and for predecessor-less states.
    members.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
    for chunk in members.chunks(MAX_COLUMN_ROWS) {
      for (row, &s) in chunk.iter().enumerate() {
        cells[s] = (next_col, row);
        row_of[s] = row as f32;
      }
      next_col += 1;
    }
  }
  cells
}

fn cubic(p: [Pos2; 4], t: f32) -> Pos2 {
  let u = 1.0 - t;
  let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
  Pos2::new(
    w[0] * p[0].x + w[1] * p[1].x + w[2] * p[2].x + w[3] * p[3].x,
    w[0] * p[0].y + w[1] * p[1].y + w[2] * p[2].y + w[3] * p[3].y,
  )
}

/// Where the ray from `rect`'s centre towards `toward` leaves the rect.
fn border_point(rect: Rect, toward: Pos2) -> Pos2 {
  let c = rect.center();
  let d = toward - c;
  if d.x == 0.0 && d.y == 0.0 {
    return c;
  }
  let sx = if d.x != 0.0 {
    rect.width() * 0.5 / d.x.abs()
  } else {
    f32::INFINITY
  };
  let sy = if d.y != 0.0 {
    rect.height() * 0.5 / d.y.abs()
  } else {
    f32::INFINITY
  };
  c + d * sx.min(sy)
}

fn dist_to_polyline(p: Pos2, pts: &[Pos2]) -> f32 {
  pts
    .windows(2)
    .map(|w| {
      let (a, b) = (w[0], w[1]);
      let ab = b - a;
      let len2 = ab.length_sq();
      let t = if len2 == 0.0 {
        0.0
      } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
      };
      p.distance(a + ab * t)
    })
    .fold(f32::INFINITY, f32::min)
}

/// Control points for the arrow `from -> to`; arrows in opposite directions bow
/// to opposite sides so they do not overlap.
fn edge_curve(from: Rect, to: Rect, bow: f32) -> [Pos2; 4] {
  if from == to {
    let top = from.center_top();
    return [
      top + Vec2::new(-12.0, 0.0),
      top + Vec2::new(-34.0, -42.0),
      top + Vec2::new(34.0, -42.0),
      top + Vec2::new(12.0, 0.0),
    ];
  }
  let a = border_point(from, to.center());
  let b = border_point(to, from.center());
  let dir = b - a;
  let normal = Vec2::new(-dir.y, dir.x).normalized() * bow;
  [a, a + dir * 0.33 + normal, a + dir * 0.66 + normal, b]
}

fn arrow_head(painter: &egui::Painter, curve: [Pos2; 4], stroke: Stroke) {
  let tip = curve[3];
  let back = (curve[2] - tip).normalized();
  let side = Vec2::new(-back.y, back.x);
  let base = tip + back * 9.0;
  painter.line_segment([tip, base + side * 4.5], stroke);
  painter.line_segment([tip, base - side * 4.5], stroke);
}

/// `recent` lists the previous states, most recent first. A status line under the canvas names the
/// hovered edge's (or node's) transitions.
pub(super) fn show_diagram(
  ui: &mut egui::Ui,
  graph: &FsmGraph,
  show_labels: bool,
  recent: [Option<usize>; HISTORY_LEN],
) {
  let n = graph.states.len();
  if n == 0 {
    ui.label("Empty state machine.");
    return;
  }
  let font = egui::TextStyle::Body.resolve(ui.style());
  let text_color = ui.visuals().text_color();
  let galleys: Vec<_> = graph
    .states
    .iter()
    .map(|s| {
      ui.painter()
        .layout_no_wrap(s.name.clone(), font.clone(), text_color)
    })
    .collect();
  let sizes: Vec<Vec2> = galleys.iter().map(|g| g.size() + NODE_PAD * 2.0).collect();

  let grid = grid_layout(graph);
  let cols = grid.iter().map(|c| c.0).max().unwrap_or(0) + 1;
  let mut col_w = vec![0.0f32; cols];
  let mut row_h = 0.0f32;
  for (i, &(c, _)) in grid.iter().enumerate() {
    col_w[c] = col_w[c].max(sizes[i].x);
    row_h = row_h.max(sizes[i].y);
  }
  let mut col_x = vec![MARGIN; cols];
  for c in 1..cols {
    col_x[c] = col_x[c - 1] + col_w[c - 1] + COL_GAP;
  }
  let base: Vec<Pos2> = grid
    .iter()
    .map(|&(c, r)| Pos2::new(col_x[c], MARGIN + 28.0 + r as f32 * (row_h + ROW_GAP)))
    .collect();

  let offsets_id = egui::Id::new(("fsm-offsets", graph.machine));
  let mut offsets: Vec<[f32; 2]> = ui
    .ctx()
    .data_mut(|d| d.get_persisted(offsets_id))
    .filter(|o: &Vec<[f32; 2]>| o.len() == n)
    .unwrap_or_else(|| vec![[0.0; 2]; n]);

  let node_pos = |i: usize, offsets: &[[f32; 2]]| {
    let p = base[i] + Vec2::new(offsets[i][0], offsets[i][1]);
    Pos2::new(p.x.max(4.0), p.y.max(4.0))
  };
  let extent = (0..n)
    .map(|i| node_pos(i, &offsets) + sizes[i])
    .fold(Pos2::ZERO, |a, b| Pos2::new(a.x.max(b.x), a.y.max(b.y)));
  let canvas = extent.to_vec2() + Vec2::splat(MARGIN);

  let edge_list = edges(graph);
  let has_reverse = |e: &Edge| edge_list.iter().any(|o| o.from == e.to && o.to == e.from);

  let mut status = String::new();
  let mut reset = false;
  egui::ScrollArea::both()
    .id_salt("fsm-diagram")
    .auto_shrink([false, false])
    .max_height(ui.available_height() - 24.0)
    .show(ui, |ui| {
      let (resp, painter) = ui.allocate_painter(canvas, Sense::hover());
      let origin = resp.rect.min.to_vec2();

      let mut rects: Vec<Rect> = Vec::with_capacity(n);
      for i in 0..n {
        let rect = Rect::from_min_size(node_pos(i, &offsets) + origin, sizes[i]);
        let r = ui.interact(
          rect,
          egui::Id::new(("fsm-node", graph.machine, i)),
          Sense::drag(),
        );
        if r.dragged() {
          let d = r.drag_delta();
          offsets[i][0] += d.x;
          offsets[i][1] += d.y;
        }
        rects.push(Rect::from_min_size(
          node_pos(i, &offsets) + origin,
          sizes[i],
        ));
        if r.hovered() {
          let outgoing: Vec<String> = edge_list
            .iter()
            .filter(|e| e.from == i)
            .flat_map(|e| e.chains.iter().map(move |&c| (e.from, c)))
            .map(|(f, c)| super::patterned_window::chain_text(graph, &graph.states[f].chains[c]))
            .collect();
          status = format!("{}: {}", graph.states[i].name, outgoing.join("   |   "));
        }
      }

      let pointer = ui.input(|i| i.pointer.hover_pos());
      let curves: Vec<[Pos2; 4]> = edge_list
        .iter()
        .map(|e| {
          edge_curve(
            rects[e.from],
            rects[e.to],
            if has_reverse(e) { 22.0 } else { 0.0 },
          )
        })
        .collect();
      let hovered_edge = pointer.and_then(|p| {
        curves
          .iter()
          .enumerate()
          .map(|(i, c)| {
            let pts: Vec<Pos2> = (0..=BEZIER_SAMPLES)
              .map(|s| cubic(*c, s as f32 / BEZIER_SAMPLES as f32))
              .collect();
            (i, dist_to_polyline(p, &pts))
          })
          .filter(|&(_, d)| d <= EDGE_HIT_RADIUS)
          .min_by(|a, b| a.1.total_cmp(&b.1))
          .map(|(i, _)| i)
      });

      let idle = ui
        .visuals()
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.55);
      let accent = Color32::LIGHT_GREEN;
      // Oldest to newest: the arrows actually taken to reach the current state.
      let path: Vec<usize> = recent
        .iter()
        .rev()
        .flatten()
        .copied()
        .chain(graph.current)
        .collect();
      let recent_rank = |s: usize| recent.iter().position(|r| *r == Some(s));
      for (i, e) in edge_list.iter().enumerate() {
        let taken_rank = path
          .windows(2)
          .any(|w| w[0] == e.from && w[1] == e.to)
          .then(|| recent_rank(e.from))
          .flatten();
        let from_current = graph.current == Some(e.from);
        let hovered = hovered_edge == Some(i);
        let stroke = if hovered {
          Stroke::new(2.5, ui.visuals().strong_text_color())
        } else if let Some(rank) = taken_rank {
          Stroke::new(2.0, RECENT_COLORS[rank])
        } else if from_current {
          Stroke::new(1.8, accent)
        } else {
          Stroke::new(1.0, idle)
        };
        let c = curves[i];
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
          c,
          false,
          Color32::TRANSPARENT,
          stroke,
        ));
        arrow_head(&painter, c, stroke);
        if show_labels || hovered {
          let label = if e.chains.len() == 1 {
            super::patterned_window::chain_conds_text(&graph.states[e.from].chains[e.chains[0]])
          } else {
            format!("{} transitions", e.chains.len())
          };
          painter.text(
            cubic(c, 0.5),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::TextStyle::Small.resolve(ui.style()),
            stroke.color,
          );
        }
        if hovered {
          let lines: Vec<String> = e
            .chains
            .iter()
            .map(|&ci| super::patterned_window::chain_text(graph, &graph.states[e.from].chains[ci]))
            .collect();
          status = format!("{}: {}", graph.states[e.from].name, lines.join("   |   "));
        }
      }

      for (i, rect) in rects.iter().enumerate() {
        let current = graph.current == Some(i);
        let rank = if current { None } else { recent_rank(i) };
        let fill = match rank {
          _ if current => Color32::from_rgb(36, 96, 54),
          Some(r) => RECENT_FILLS[r],
          None => ui.visuals().extreme_bg_color,
        };
        let border = match rank {
          _ if current => accent,
          Some(r) => RECENT_COLORS[r],
          None => ui.visuals().widgets.noninteractive.bg_stroke.color,
        };
        painter.rect_filled(*rect, 5.0, fill);
        painter.rect_stroke(
          *rect,
          5.0,
          Stroke::new(1.5, border),
          egui::StrokeKind::Inside,
        );
        let color = if current || rank.is_some() {
          Color32::WHITE
        } else {
          text_color
        };
        painter.galley(rect.min + NODE_PAD, galleys[i].clone(), color);
      }

      let rr = ui.put(
        Rect::from_min_size(resp.rect.min + Vec2::new(6.0, 4.0), Vec2::new(90.0, 20.0)),
        egui::Button::new("Reset layout").small(),
      );
      reset = rr.clicked();
    });

  if reset {
    offsets = vec![[0.0; 2]; n];
  }
  ui.ctx()
    .data_mut(|d| d.insert_persisted(offsets_id, offsets));
  ui.label(if status.is_empty() {
    "Green: current, yellow: previous two states. Drag nodes to rearrange; hover an arrow or node for its transitions.".to_string()
  } else {
    status
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mem::ai_fsm::{FsmChain, FsmCond, FsmState};

  fn graph(links: &[&[usize]]) -> FsmGraph {
    FsmGraph {
      machine: 0,
      current: None,
      states: links
        .iter()
        .enumerate()
        .map(|(i, targets)| FsmState {
          name: format!("S{i}"),
          chains: targets
            .iter()
            .map(|&t| FsmChain {
              conds: vec![FsmCond {
                name: "Default".into(),
                arg: 0.0,
                negated: false,
              }],
              target: Some(t),
            })
            .collect(),
        })
        .collect(),
    }
  }

  #[test]
  fn layers_follow_bfs_depth_from_every_root() {
    // 0 -> 1,2 ; 1 -> 3 ; 2 -> 3 ; 4 -> 0 has no way in, so it is a root too.
    let g = graph(&[&[1, 2], &[3], &[3], &[], &[0]]);
    let cells = grid_layout(&g);
    assert_eq!(cells[0], (0, 0));
    assert_eq!(cells[4], (0, 1));
    assert_eq!(cells[1], (1, 0));
    assert_eq!(cells[2], (1, 1));
    assert_eq!(cells[3], (2, 0));
  }

  #[test]
  fn states_only_entered_from_code_sit_on_the_left() {
    // 0 -> 1 ; 2 -> 3 and 3 -> 4 are only reachable from code.
    let g = graph(&[&[1], &[], &[3], &[4], &[]]);
    let cells = grid_layout(&g);
    assert_eq!(cells[0].0, 0);
    assert_eq!(cells[2].0, 0);
    assert_eq!(cells[3].0, 1);
    assert_eq!(cells[4].0, 2);
  }

  #[test]
  fn an_unreachable_cycle_still_gets_a_root() {
    // 1 <-> 2 has no incoming edge from outside the cycle.
    let g = graph(&[&[], &[2], &[1]]);
    let cells = grid_layout(&g);
    assert_eq!(cells[1].0, 0);
    assert_eq!(cells[2].0, 1);
  }

  #[test]
  fn tall_columns_wrap_instead_of_growing_forever() {
    let targets: Vec<usize> = (1..=25).collect();
    let mut links: Vec<&[usize]> = vec![&targets];
    links.extend(std::iter::repeat_n(&[][..], 25));
    let cells = grid_layout(&graph(&links));
    assert!(cells.iter().all(|&(_, r)| r < MAX_COLUMN_ROWS));
    let cols: std::collections::BTreeSet<_> = cells.iter().map(|c| c.0).collect();
    assert_eq!(cols.len(), 4); // root + ceil(25 / 10) wrapped columns
  }

  #[test]
  fn parallel_transitions_collapse_into_one_edge() {
    let g = graph(&[&[1, 1, 0], &[]]);
    let e = edges(&g);
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].chains, vec![0, 1]);
    assert_eq!((e[1].from, e[1].to), (0, 0));
  }

  #[test]
  fn border_point_lands_on_the_rect_edge() {
    let r = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 40.0));
    let p = border_point(r, Pos2::new(300.0, 20.0));
    assert!((p.x - 100.0).abs() < 1e-3 && (p.y - 20.0).abs() < 1e-3);
  }
}
