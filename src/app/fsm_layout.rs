//! Placing a state machine's nodes: a BFS-layered grid, then a force-directed
//! relaxation seeded from it. Pure functions of the graph and node sizes.

use std::collections::VecDeque;

use egui::{Pos2, Rect, Vec2};

use crate::mem::ai_fsm::FsmGraph;

pub(super) const MARGIN: f32 = 30.0;
/// Room above the first row for the "Reset layout" button.
pub(super) const TOP_INSET: f32 = 28.0;
const COL_GAP: f32 = 110.0;
const ROW_GAP: f32 = 38.0;

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

/// Top-left position per node for the layered grid: columns from `grid_layout`,
/// spaced by the widest node in each.
pub(super) fn grid_positions(graph: &FsmGraph, sizes: &[Vec2]) -> Vec<Pos2> {
  let grid = grid_layout(graph);
  let cols = grid.iter().map(|c| c.0).max().map_or(0, |m| m + 1);
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
  grid
    .iter()
    .map(|&(c, r)| Pos2::new(col_x[c], MARGIN + TOP_INSET + r as f32 * (row_h + ROW_GAP)))
    .collect()
}

const FORCE_ITERATIONS: usize = 400;
/// Strength of the pull keeping arrows pointing left to right.
const DIRECTION_BIAS: f32 = 0.35;
const GRAVITY: f32 = 0.02;
const OVERLAP_GAP: Vec2 = Vec2::new(18.0, 12.0);

/// Fruchterman-Reingold relaxation: every pair of nodes repels, every arrow pulls
/// its ends together, and arrows that point backwards are nudged rightwards so the
/// diagram still reads left to right. Starts from `start` (top-left positions,
/// normally [`grid_positions`]) and cools linearly, so it is deterministic and
/// always terminates after [`FORCE_ITERATIONS`] steps. Returns top-left positions
/// with no overlapping nodes, shifted so the top-left of the drawing is at the
/// margin.
pub(super) fn force_layout(graph: &FsmGraph, sizes: &[Vec2], start: &[Pos2]) -> Vec<Pos2> {
  let n = graph.states.len();
  if n < 2 {
    return start.to_vec();
  }
  let mean_diag = sizes.iter().map(|s| s.length()).sum::<f32>() / n as f32;
  let k = mean_diag * 1.7;
  let links: Vec<(usize, usize)> = edges(graph)
    .iter()
    .filter(|e| e.from != e.to)
    .map(|e| (e.from, e.to))
    .collect();

  // The tiny index-based nudge keeps two nodes seeded on the same spot apart.
  let mut pos: Vec<Vec2> = (0..n)
    .map(|i| (start[i] + sizes[i] * 0.5).to_vec2() + Vec2::splat((i % 7) as f32 * 0.01))
    .collect();

  for it in 0..FORCE_ITERATIONS {
    let temperature = k * (1.0 - it as f32 / FORCE_ITERATIONS as f32) + 1.0;
    let mut disp = vec![Vec2::ZERO; n];
    for i in 0..n {
      for j in (i + 1)..n {
        let d = pos[i] - pos[j];
        let dist = d.length().max(1.0);
        let push = d / dist * (k * k / dist);
        disp[i] += push;
        disp[j] -= push;
      }
    }
    for &(u, v) in &links {
      let d = pos[v] - pos[u];
      let dist = d.length().max(1.0);
      let pull = d / dist * (dist * dist / k);
      disp[u] += pull;
      disp[v] -= pull;
      let short_of_right = k * 0.6 - d.x;
      if short_of_right > 0.0 {
        let shove = short_of_right * DIRECTION_BIAS;
        disp[v].x += shove;
        disp[u].x -= shove;
      }
    }
    let centroid = pos.iter().fold(Vec2::ZERO, |a, p| a + *p) / n as f32;
    for i in 0..n {
      disp[i] -= (pos[i] - centroid) * GRAVITY * k;
      let len = disp[i].length();
      if len > 0.0 {
        pos[i] += disp[i] / len * len.min(temperature);
      }
    }
  }

  let mut tl: Vec<Pos2> = (0..n)
    .map(|i| (pos[i] - sizes[i] * 0.5).to_pos2())
    .collect();
  separate_overlaps(&mut tl, sizes);
  let min = tl.iter().fold(Pos2::new(f32::MAX, f32::MAX), |a, p| {
    Pos2::new(a.x.min(p.x), a.y.min(p.y))
  });
  let shift = Vec2::new(MARGIN, MARGIN + TOP_INSET) - min.to_vec2();
  tl.iter().map(|p| *p + shift).collect()
}

/// Pushes overlapping nodes apart along whichever axis needs the smaller move.
fn separate_overlaps(tl: &mut [Pos2], sizes: &[Vec2]) {
  let n = tl.len();
  for _ in 0..200 {
    let mut moved = false;
    for i in 0..n {
      for j in (i + 1)..n {
        let a = Rect::from_min_size(tl[i], sizes[i]).expand2(OVERLAP_GAP * 0.5);
        let b = Rect::from_min_size(tl[j], sizes[j]).expand2(OVERLAP_GAP * 0.5);
        let overlap = a.intersect(b);
        if overlap.width() <= 0.0 || overlap.height() <= 0.0 {
          continue;
        }
        moved = true;
        let d = b.center() - a.center();
        let shift = if overlap.width() < overlap.height() {
          Vec2::new(
            overlap.width() * 0.5 * if d.x < 0.0 { -1.0 } else { 1.0 },
            0.0,
          )
        } else {
          Vec2::new(
            0.0,
            overlap.height() * 0.5 * if d.y < 0.0 { -1.0 } else { 1.0 },
          )
        };
        tl[i] -= shift;
        tl[j] += shift;
      }
    }
    if !moved {
      break;
    }
  }
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
          address: i as u32,
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

  fn sizes_for(g: &FsmGraph) -> Vec<Vec2> {
    g.states
      .iter()
      .map(|s| Vec2::new(8.0 * s.name.len() as f32 + 20.0, 26.0))
      .collect()
  }

  fn rects(pos: &[Pos2], sizes: &[Vec2]) -> Vec<Rect> {
    pos
      .iter()
      .zip(sizes)
      .map(|(p, s)| Rect::from_min_size(*p, *s))
      .collect()
  }

  /// A hub with `spokes` leaves, each of which loops back to the hub.
  fn hub(spokes: usize) -> FsmGraph {
    let targets: Vec<usize> = (1..=spokes).collect();
    let mut links: Vec<&[usize]> = vec![&targets];
    links.extend(std::iter::repeat_n(&[0usize][..], spokes));
    graph(&links)
  }

  #[test]
  fn force_layout_is_deterministic() {
    let g = hub(12);
    let sizes = sizes_for(&g);
    let start = grid_positions(&g, &sizes);
    assert_eq!(
      force_layout(&g, &sizes, &start),
      force_layout(&g, &sizes, &start)
    );
  }

  #[test]
  fn force_layout_never_overlaps_nodes() {
    let g = hub(30);
    let sizes = sizes_for(&g);
    let pos = force_layout(&g, &sizes, &grid_positions(&g, &sizes));
    let r = rects(&pos, &sizes);
    for i in 0..r.len() {
      for j in (i + 1)..r.len() {
        assert!(!r[i].intersects(r[j]), "{i} overlaps {j}");
      }
    }
    assert!(
      pos
        .iter()
        .all(|p| p.x >= MARGIN - 0.5 && p.y >= MARGIN - 0.5)
    );
  }

  #[test]
  fn force_layout_pulls_connected_nodes_together() {
    // Two separate 6-cycles: arrows within a cycle must be shorter than the gap
    // between the cycles.
    let mut links: Vec<Vec<usize>> = Vec::new();
    for base in [0usize, 6] {
      for i in 0..6 {
        links.push(vec![base + (i + 1) % 6]);
      }
    }
    let refs: Vec<&[usize]> = links.iter().map(|v| v.as_slice()).collect();
    let g = graph(&refs);
    let sizes = sizes_for(&g);
    let pos = force_layout(&g, &sizes, &grid_positions(&g, &sizes));
    let center = |i: usize| pos[i] + sizes[i] * 0.5;
    let mean = |pairs: Vec<(usize, usize)>| {
      pairs
        .iter()
        .map(|&(a, b)| center(a).distance(center(b)))
        .sum::<f32>()
        / pairs.len() as f32
    };
    let within: Vec<_> = (0..6).map(|i| (i, (i + 1) % 6)).collect();
    let across: Vec<_> = (0..6).map(|i| (i, 6 + i)).collect();
    assert!(mean(within) < mean(across));
  }

  fn segments_cross(a: (Pos2, Pos2), b: (Pos2, Pos2)) -> bool {
    let orient = |p: Pos2, q: Pos2, r: Pos2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2) = (orient(b.0, b.1, a.0), orient(b.0, b.1, a.1));
    let (d3, d4) = (orient(a.0, a.1, b.0), orient(a.0, a.1, b.1));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
  }

  /// (total arrow length, crossings between arrows that share no node).
  fn metrics(g: &FsmGraph, pos: &[Pos2], sizes: &[Vec2]) -> (f32, usize) {
    let c = |i: usize| pos[i] + sizes[i] * 0.5;
    let es: Vec<(usize, usize)> = edges(g)
      .iter()
      .filter(|e| e.from != e.to)
      .map(|e| (e.from, e.to))
      .collect();
    let len = es.iter().map(|&(a, b)| c(a).distance(c(b))).sum();
    let mut crossings = 0;
    for i in 0..es.len() {
      for j in (i + 1)..es.len() {
        let (a, b) = (es[i], es[j]);
        if a.0 == b.0 || a.0 == b.1 || a.1 == b.0 || a.1 == b.1 {
          continue;
        }
        if segments_cross((c(a.0), c(a.1)), (c(b.0), c(b.1))) {
          crossings += 1;
        }
      }
    }
    (len, crossings)
  }

  /// Lays out every big `CPatterned` machine in the Exo dump both ways, prints
  /// metrics, and writes `$FSM_LAYOUT_OUT` (JSON) for rendering. Run manually.
  #[test]
  #[ignore]
  fn exo_layout_report() {
    use crate::ctx::Ctx;
    use crate::mem::ai_fsm::read_fsm;
    use crate::mem::game_memory::GameMemory;
    use crate::mem::game_object_utils::get_all_objects;
    use crate::mem::game_version::GameVersion;
    use crate::structs::prime_structs::GameStructs;

    let mut structs = GameStructs::new_empty();
    structs
      .load_from_dir(
        concat!(env!("CARGO_MANIFEST_DIR"), "/prime_defs"),
        GameVersion::default(),
      )
      .unwrap();
    let mut mem = GameMemory::new();
    mem
      .load_from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/mem1_exo_room1.raw"))
      .unwrap();
    let ctx = Ctx::new(&structs, &mem, GameVersion::default());
    for e in get_all_objects(&ctx).values() {
      if !e.extends_class(&ctx, "CPatterned") {
        continue;
      }
      let Some(g) = read_fsm(&ctx, e) else { continue };
      if g.states.len() < 20 {
        continue;
      }
      let sizes = sizes_for(&g);
      let grid = grid_positions(&g, &sizes);
      let t = std::time::Instant::now();
      let force = force_layout(&g, &sizes, &grid);
      let took = t.elapsed();
      let extent = |p: &[Pos2]| {
        let r = p.iter().zip(&sizes).fold(Rect::NOTHING, |r, (p, s)| {
          r.union(Rect::from_min_size(*p, *s))
        });
        (r.width().round(), r.height().round())
      };
      eprintln!(
        "{}: {} states\n  grid : (len, crossings) = {:?} extent {:?}\n  force: (len, crossings) = {:?} extent {:?} in {took:?}",
        e.type_name,
        g.states.len(),
        metrics(&g, &grid, &sizes),
        extent(&grid),
        metrics(&g, &force, &sizes),
        extent(&force),
      );
      if let Ok(out) = std::env::var("FSM_LAYOUT_OUT") {
        let j = |p: &[Pos2]| {
          p.iter()
            .map(|p| format!("[{:.1},{:.1}]", p.x, p.y))
            .collect::<Vec<_>>()
            .join(",")
        };
        let names = g
          .states
          .iter()
          .map(|s| format!("{:?}", s.name))
          .collect::<Vec<_>>()
          .join(",");
        let sz = sizes
          .iter()
          .map(|s| format!("[{:.1},{:.1}]", s.x, s.y))
          .collect::<Vec<_>>()
          .join(",");
        let es = edges(&g)
          .iter()
          .map(|e| format!("[{},{}]", e.from, e.to))
          .collect::<Vec<_>>()
          .join(",");
        std::fs::write(
          out,
          format!(
            "{{\"names\":[{names}],\"sizes\":[{sz}],\"edges\":[{es}],\"current\":{},\"grid\":[{}],\"force\":[{}]}}",
            g.current.map_or(-1, |c| c as i64),
            j(&grid),
            j(&force)
          ),
        )
        .unwrap();
      }
      break;
    }
  }
}
