//! `primewatch3 --trace-player`: headless CSV recorder for the player's per-frame
//! physics state, read from Dolphin's shared memory. For debugging
//! `scripts/aether_depth.rhai` against real frames (frame-advance in Dolphin).
//!
//! Dolphin's frames aren't observable from outside, so this polls fast and emits
//! a row whenever the tracked state changes. Under frame advance that is one row
//! per emulated frame. A frame that changes nothing produces no row, and at
//! full speed a poll can straddle two frames and merge them, so use frame
//! advance.

use std::fmt::Write as _;
use std::io::Write as _;
use std::time::{Duration, Instant};

use crate::app::{DEFS_DIR, load_defs};
use crate::ctx::Ctx;
use crate::mem::dolphin_memory::DolphinMemoryAccess;
use crate::mem::game_memory::GameMemory;
use crate::mem::game_version::GameVersion;
use crate::mem::globals::{get_state_manager, get_tweak_player};
use crate::mem::math_utils::{read_as_transform, read_as_vec3};
use crate::structs::prime_structs::{GameInstance, GameStructs};

const USAGE: &str = "usage: primewatch3 --trace-player [--rows N] [--out FILE] [--poll-us N]\n\
  Attach to Dolphin and append a CSV row each time the player's state changes.\n\
  Defaults: --rows 2000, --out player_trace.csv, --poll-us 500. Ctrl+C stops early;\n\
  rows are flushed as they are written.";

struct Options {
  rows: usize,
  out: String,
  poll: Duration,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
  let mut opts = Options {
    rows: 2000,
    out: "player_trace.csv".to_string(),
    poll: Duration::from_micros(500),
  };
  let mut it = args.iter();
  while let Some(arg) = it.next() {
    let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
    match arg.as_str() {
      "--rows" => {
        opts.rows = value("--rows")?
          .parse()
          .map_err(|e| format!("--rows: {e}"))?
      }
      "--out" => opts.out = value("--out")?.clone(),
      "--poll-us" => {
        let us: u64 = value("--poll-us")?
          .parse()
          .map_err(|e| format!("--poll-us: {e}"))?;
        opts.poll = Duration::from_micros(us);
      }
      other => return Err(format!("unknown argument {other}")),
    }
  }
  Ok(opts)
}

/// Run if `--trace-player` is among the process args. `None` means "not
/// requested; start the GUI".
pub fn run_if_requested() -> Option<Result<(), String>> {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let pos = args.iter().position(|a| a == "--trace-player")?;
  let mut rest = args;
  rest.remove(pos);
  if rest.iter().any(|a| a == "--help" || a == "-h") {
    println!("{USAGE}");
    return Some(Ok(()));
  }
  Some(parse_options(&rest).and_then(|o| trace(&o)))
}

fn attach(dolphin: &mut DolphinMemoryAccess) -> Result<(), String> {
  let pids = dolphin.get_dolphin_pids();
  match pids.as_slice() {
    [] => Err("no Dolphin process found".into()),
    [pid] => {
      if dolphin.attach_to_process(pid.as_u32() as i32) {
        Ok(())
      } else {
        Err(format!("failed to attach to Dolphin pid {pid}"))
      }
    }
    many => Err(format!(
      "{} Dolphin processes found; close all but one",
      many.len()
    )),
  }
}

struct PlayerSample {
  pos: glam::Vec3,
  vel: glam::Vec3,
  momentum: glam::Vec3,
  force: glam::Vec3,
  impulse: glam::Vec3,
  movement_state: u32,
  morph_state: u32,
  falling_time: f32,
  step_up: f32,
  step_down: f32,
  sliding_on_wall: bool,
  in_water: bool,
}

impl PlayerSample {
  fn read(ctx: &Ctx, player: &GameInstance) -> Option<Self> {
    let vec = |name: &str| read_as_vec3(ctx, &player.get_member(ctx, name)?);
    Some(Self {
      pos: read_as_transform(ctx, &player.get_member(ctx, "transform")?)?
        .w_axis
        .truncate(),
      vel: vec("velocity")?,
      momentum: vec("momentum")?,
      force: vec("force")?,
      impulse: vec("impulse")?,
      movement_state: player.get_member(ctx, "movementState")?.read_u32(ctx)?,
      morph_state: player.get_member(ctx, "morphState")?.read_u32(ctx)?,
      falling_time: player.get_member(ctx, "fallingTime")?.read_f32(ctx)?,
      step_up: player.get_member(ctx, "stepUpHeight")?.read_f32(ctx)?,
      step_down: player.get_member(ctx, "stepDownHeight")?.read_f32(ctx)?,
      sliding_on_wall: player.get_member(ctx, "slidingOnWall")?.read_bool(ctx)?,
      in_water: player.get_member(ctx, "inWaterMovement")?.read_bool(ctx)?,
    })
  }

  /// Bit-exact: f32 rounding is exactly what we want to see.
  fn key(&self) -> [u32; 14] {
    [
      self.pos.x.to_bits(),
      self.pos.y.to_bits(),
      self.pos.z.to_bits(),
      self.vel.x.to_bits(),
      self.vel.y.to_bits(),
      self.vel.z.to_bits(),
      self.momentum.z.to_bits(),
      self.force.z.to_bits(),
      self.impulse.z.to_bits(),
      self.movement_state,
      self.morph_state,
      self.falling_time.to_bits(),
      self.step_up.to_bits(),
      self.step_down.to_bits(),
    ]
  }
}

struct Area {
  lo: glam::Vec3,
  hi: glam::Vec3,
  chain: i32,
}

fn read_areas(ctx: &Ctx) -> Vec<Area> {
  let mut areas = Vec::new();
  let Some(world) = get_state_manager(ctx).and_then(|sm| sm.get_member(ctx, "world")) else {
    return areas;
  };
  let Some(list) = world.get_member(ctx, "areas") else {
    return areas;
  };
  let count = list
    .get_member(ctx, "count")
    .and_then(|c| c.read_u32(ctx))
    .unwrap_or(0);
  let Some(first) = list.get_member(ctx, "first") else {
    return areas;
  };
  for i in 0..count {
    let Some(area) = first.element(ctx, i).get_member(ctx, "value") else {
      continue;
    };
    let chain = area
      .get_member(ctx, "curChain")
      .and_then(|c| c.read_u32(ctx))
      .map(|c| c as i32);
    let bounds = |name: &str| {
      let aabb = area.get_member(ctx, "aabb")?;
      read_as_vec3(ctx, &aabb.get_member(ctx, name)?)
    };
    if let (Some(chain), Some(lo), Some(hi)) = (chain, bounds("min"), bounds("max")) {
      areas.push(Area { lo, hi, chain });
    }
  }
  areas
}

/// Inclusive overlap, as `CAABox::DoBoundsOverlap`.
fn overlaps(lo: glam::Vec3, hi: glam::Vec3, area: &Area) -> bool {
  lo.cmple(area.hi).all() && area.lo.cmple(hi).all()
}

fn trace(opts: &Options) -> Result<(), String> {
  let mut dolphin = DolphinMemoryAccess::new();
  attach(&mut dolphin)?;
  let mut mem = GameMemory::new();
  mem.update_from_dolphin(&mut dolphin);
  let version = GameVersion::detect(&mem)
    .ok_or("Dolphin has no recognised Prime game loaded (is it running?)")?;

  let mut structs = GameStructs::new_empty();
  load_defs(&mut structs, version).map_err(|e| format!("load {DEFS_DIR}: {e}"))?;

  let mut file = std::io::BufWriter::new(
    std::fs::File::create(&opts.out).map_err(|e| format!("create {}: {e}", opts.out))?,
  );
  eprintln!(
    "tracing to {} ({} rows max); step frames in Dolphin. Ctrl+C to stop.",
    opts.out, opts.rows
  );

  let started = Instant::now();
  let mut last_key: Option<[u32; 14]> = None;
  let mut written = 0usize;
  let mut header_done = false;

  while written < opts.rows {
    mem.update_from_dolphin(&mut dolphin);
    let ctx = Ctx::new(&structs, &mem, version);
    let sample = get_state_manager(&ctx)
      .and_then(|sm| sm.get_member(&ctx, "player"))
      .and_then(|p| PlayerSample::read(&ctx, &p).map(|s| (p, s)));
    let Some((player, s)) = sample else {
      std::thread::sleep(opts.poll);
      continue;
    };
    if last_key == Some(s.key()) {
      std::thread::sleep(opts.poll);
      continue;
    }
    last_key = Some(s.key());

    let areas = read_areas(&ctx);
    let (local_lo, local_hi) = touch_offsets(&ctx, &player).unwrap_or_default();

    if !header_done {
      write_header(&mut file, &ctx, &areas, local_lo, local_hi).map_err(|e| e.to_string())?;
      header_done = true;
    }

    let lo = s.pos + local_lo;
    let hi = s.pos + local_hi;
    let in_bounds = areas
      .iter()
      .filter(|a| a.chain == 3)
      .any(|a| overlaps(lo, hi, a));
    let mut line = String::new();
    let _ = write!(
      line,
      "{written},{},{},{},{},{:#010x},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
      started.elapsed().as_millis(),
      s.pos.x,
      s.pos.y,
      s.pos.z,
      s.pos.z.to_bits(),
      s.vel.x,
      s.vel.y,
      s.vel.z,
      s.momentum.z,
      s.force.z,
      s.impulse.z,
      s.movement_state,
      s.morph_state,
      s.falling_time,
      s.step_up,
      s.step_down,
      s.sliding_on_wall as u8,
      s.in_water as u8,
      in_bounds as u8,
    );
    writeln!(file, "{line}").map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    written += 1;
    std::thread::sleep(opts.poll);
  }
  eprintln!("wrote {written} rows to {}", opts.out);
  Ok(())
}

/// Touch bounds relative to the player origin (unmorphed), matching the script.
fn touch_offsets(ctx: &Ctx, player: &GameInstance) -> Option<(glam::Vec3, glam::Vec3)> {
  let off = read_as_vec3(ctx, &player.get_member(ctx, "primitiveOffset")?)?;
  let bb = player.get_member(ctx, "baseBoundingBox")?;
  let lo = read_as_vec3(ctx, &bb.get_member(ctx, "min")?)?;
  let hi = read_as_vec3(ctx, &bb.get_member(ctx, "max")?)?;
  Some((lo + off, hi + off))
}

fn write_header(
  out: &mut impl std::io::Write,
  ctx: &Ctx,
  areas: &[Area],
  local_lo: glam::Vec3,
  local_hi: glam::Vec3,
) -> std::io::Result<()> {
  let gravity = get_tweak_player(ctx)
    .and_then(|t| t.get_member(ctx, "normalGravAccel"))
    .and_then(|g| g.read_f32(ctx));
  writeln!(out, "# version {}", ctx.version.id())?;
  writeln!(out, "# gravity {gravity:?}")?;
  writeln!(out, "# touch_local_lo {local_lo:?}")?;
  writeln!(out, "# touch_local_hi {local_hi:?}")?;
  for (i, a) in areas.iter().enumerate() {
    writeln!(
      out,
      "# area {i} chain={} lo={:?} hi={:?}",
      a.chain, a.lo, a.hi
    )?;
  }
  writeln!(
    out,
    "row,ms,x,y,z,z_bits,vx,vy,vz,momentum_z,force_z,impulse_z,movement_state,morph_state,\
     falling_time,step_up,step_down,sliding_on_wall,in_water,in_bounds"
  )
}
