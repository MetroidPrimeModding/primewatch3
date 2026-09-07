# Collision ray tracing — `RayWorldIntersection`

Research notes for recreating Metroid Prime's world ray cast in `primewatch3`, sourced
from `metaforce` (`../metaforce`, a non-matching PC port — variable names and a few
epsilon constants may differ slightly, but the control flow and math are faithful).

The goal: given a world-space ray `(pos, dir, length)` — `dir` **unit length**, `length`
in world units — return the nearest surface hit: `t` (distance), hit point, surface
plane, and the surface material word. This is what the game uses to (a) clamp a
physics actor's motion each frame (`CGameCollision::MoveAndCollide`), (b) do
line-of-sight / weapon / camera checks.

--------------------------------------------------------------------------------
## 1. Call graph

```
CStateManager::RayWorldIntersection(idOut, pos, dir, length, filter, nearList)
└─ CGameCollision::RayWorldIntersection                       [CGameCollision.cpp:347]
   ├─ RayStaticIntersection(mgr, pos, dir, length, filter)    [CGameCollision.cpp:249]  ← world geometry (octree)
   │  └─ for each CGameArea:  area.postConstructed.collision (CAreaOctTree)
   │     └─ CAreaOctTree::Node::LineTestEx(line, filter, rayRes, length)  [CAreaOctTree.cpp:415]
   │        └─ LineTestExInternal(...)  recursive octree descent          [CAreaOctTree.cpp:203]
   │           └─ per-leaf: Möller–Trumbore vs every triangle in the leaf
   └─ RayDynamicIntersection(mgr, idOut, pos, dir, length, filter, nearList) [CGameCollision.cpp:295]  ← actors
      └─ for each actor id in nearList:
         prim->CastRay(pos, dir, bestT, filter, primitiveXf)   (sphere / AABox / OBB-tree)
```

`RayWorldIntersection` runs **both** halves and keeps whichever hit is nearer
(§6). For the primetwatch use-case (visualising what the game's world collision
does) the **static** half is the important part; the dynamic half is lower
priority and needs more schema (§8).

--------------------------------------------------------------------------------
## 2. `CRayCastResult`  (`CRayCastResult.hpp`)

```
float            t          // distance along dir (dir is unit length)
CVector3f        point      // pos + dir*t
CPlane           plane      // surface plane: (normal.xyz, d) with d = normal · vert0
EInvalid         invalid    // Invalid | Valid   -> IsValid()
CMaterialList    material   // 64-bit; for a static hit == the triangle's 32-bit surface word
```

An "invalid" result = no hit. `MakeInvalid()` only flips the flag (leaves the
other fields stale) — so always gate on `IsValid()`.

--------------------------------------------------------------------------------
## 3. `RayStaticIntersection`  (`CGameCollision.cpp:249-275`)

```
bestT = length;  if (bestT <= 0) bestT = 100000;
line = CLine(pos, dir);
result = invalid;
for (area : world) {
    CAreaOctTree::SRayResult rr;                 // { CPlane plane; optional<CCollisionSurface> surface; float t; }
    area.collision.GetRootNode().LineTestEx(line, filter, rr, length);
    if (!rr.surface) continue;
    if (length != 0 && length < rr.t) continue;  // hit is past the requested length
    if (rr.t < bestT) {
        result = CRayCastResult(rr.t, pos + dir*rr.t, rr.plane, rr.surface.GetSurfaceFlags());
        bestT = rr.t;
    }
}
return result;
```

Notes
* Iterates **every loaded area**. In practice one area is active; `primewatch3`
  can iterate whatever `get_areas()` returns.
* `length` is passed straight through to `LineTestEx` as its `maxT`. The
  `length < rr.t` re-check afterwards is redundant belt-and-suspenders.
* `rr.plane` is set from the winning triangle (`surface.GetPlane()`), see §5.3.

### `SRayResult`
```
CPlane                      x0_plane   // filled from surface->GetPlane() once a triangle wins
optional<CCollisionSurface> x10_surface
float                       x3c_t
```

--------------------------------------------------------------------------------
## 4. The octree — `CAreaOctTree`

### 4.1 Struct (already in `prime_defs/prime1/CAreaOctTree.bs`)

| member       | off  | meaning |
|--------------|------|---------|
| `aabb`       | 0x00 | `CAABB` — octree root bounds (**use this**, not `CGameArea.aabb`) |
| `treeType`   | 0x18 | `ETreeType` of the **root** node: `Invalid=0, Branch=1, Leaf=2` |
| `buf`        | 0x1C | base of the file blob (`buf + 8` is the internal base) |
| `treebuff`   | 0x20 | pointer to the **root node body** (the bytes traversal walks) |
| `matCount`   | 0x24 | material-word count |
| `materials`  | 0x28 | `*u32` — array of surface material words |
| `vertMats`   | 0x2C | `*u8` — per-vertex material index |
| `edgeMats`   | 0x30 | `*u8` — per-edge material index |
| `polyMats`   | 0x34 | `*u8` — per-triangle material index → `materials[polyMats[i]]` |
| `edgeCount`  | 0x38 | |
| `edges`      | 0x3C | `*CCollisionEdge` — each is `{u16 vi1, u16 vi2}` |
| `polyCount`  | 0x40 | triangle count |
| `polyEdges`  | 0x44 | `*u16` — 3 edge indices per triangle (`polyEdges[i*3 + {0,1,2}]`) |
| `vertCount`  | 0x48 | |
| `verts`      | 0x4C | `*f32` — xyz triplets |

`primewatch3`'s `collision_mesh.rs::load_mesh` already walks
`area → postConstructed → collision["value"]` and copies all of these arrays out;
the ray tracer can reuse that path / that data.

### 4.2 The tree-node binary format (hand-parsed — **no `.bs` possible**)

The node bytes are a bespoke variable-length format that the C++ walks with raw
pointer casts (`CAreaOctTree::Node::GetChild`, `SwapTreeNode`). All multi-byte
fields are **big-endian in the dump** (the game byte-swaps them in place at load;
we read straight from the dump, so swap on read).

**Branch node** — header is 36 bytes:

| bytes      | field |
|------------|-------|
| `[0x00..0x02)` | `u16 childFlags` — 8 × 2-bit child types, `type(i) = (childFlags >> (2*i)) & 3` |
| `[0x02..0x04)` | padding |
| `[0x04..0x24)` | `u32 offset[8]` — child `i` body starts at `nodeStart + 36 + offset[i]` |

Child `i`'s **AABB is not stored** — it is the `i`-th octant of the parent AABB.
Octant bit layout: bit0 = X, bit1 = Y, bit2 = Z (0 = negative/low half, 1 =
positive/high half). Splitting matches `zeus::CAABox::splitX/Y/Z`: the mid plane
is the exact arithmetic midpoint `min + (max-min)*0.5` per axis. (C++ order:
splitZ, then splitY, then splitX — result is identical to independent per-axis
midpoint split.)

**Leaf node**:

| bytes           | field |
|-----------------|-------|
| `[0x00..0x18)`  | `f32 aabb[6]` — `minX,minY,minZ, maxX,maxY,maxZ` (leaf AABB **is** stored) |
| `[0x18..0x1A)`  | `u16 triCount` |
| `[0x1A..0x1A+2*triCount)` | `u16 triIndex[triCount]` — indices into the master triangle list |

`GetChild(i)`:
* `type = (childFlags >> 2*i) & 3`
* `Branch` → node body at `nodeStart + 36 + offset[i]`, AABB = octant `i` of parent
* `Leaf`   → leaf body at `nodeStart + 36 + offset[i]`, AABB = the 6 stored floats
* `Invalid` → empty, skip

Root node: body at `treebuff`, AABB = `CAreaOctTree.aabb`, type = `treeType`.

**Special case:** `childFlags == 0x000A` exactly (= children 0 and 1 both `Leaf`,
2..7 `Invalid`) is handled by a simpler "two leaves" path (§4.4).

### 4.3 `BoxLineTest` — ray/AABB slab test  (`CAreaOctTree.cpp:18-54`)

Returns whether `line` crosses `aabb`, and the entry/exit parameters `lT`/`hT`
(may be negative). Per axis `i` in 0..3:

```
EPS = 0.000099999997          // ~1e-4
if |dir[i]| <= EPS:                       // ray parallel to this slab
    if origin[i] < min[i] || origin[i] > max[i]: return false
if dir[i] < 0:
    if max[i]-origin[i]  <  lT*dir[i]:  lT = (max[i]-origin[i]) / dir[i]
    if min[i]-origin[i]  >  hT*dir[i]:  hT = (min[i]-origin[i]) / dir[i]
else:
    if min[i]-origin[i]  >  lT*dir[i]:  lT = (min[i]-origin[i]) / dir[i]
    if max[i]-origin[i]  <  hT*dir[i]:  hT = (max[i]-origin[i]) / dir[i]
```
Init `lT = -FLT_MAX, hT = +FLT_MAX`. Return `lT <= hT`.
(The comparisons are written pre-divide `a < lT*dir` so the branch works for
negative `dir`; the effect is the standard slab min/max.)

### 4.4 `LineTestExInternal` — the recursive descent  (`CAreaOctTree.cpp:203-400`)

Signature: `(line, filter, SRayResult& res, float lT, float hT, float maxT, CVector3f dirRecip)`
where `dirRecip = 1/dir` component-wise. Entry from `LineTestEx`:

```
LineTestEx(line, filter, res, length):
    if root.type == Invalid: return
    if !BoxLineTest(root.aabb, line, lT, hT): return
    dirRecip = 1 / line.dir                      // component-wise
    LineTestExInternal(line, filter, res, lT - 1e-4, hT + 1e-4, length, dirRecip)
```

**Per-call clamp** (top of `LineTestExInternal`):
```
FUDGE = FLT_EPSILON * 100        // ~1.192e-5
lowT  = (1 - FUDGE) * lT
highT = (1 + FUDGE) * hT
if maxT != 0:
    lowT  = max(lowT, 0)
    highT = min(highT, maxT)
    if lowT > highT: return
```

**Leaf case:** run the triangle test (§5) against every `triIndex`, keeping the
smallest `t` in `[lowT, highT)` whose material passes the filter. If one is found,
overwrite `res` with `{ surface, t }` and set `res.plane = surface.GetPlane()`.

**Branch case, general:** front-to-back octant walk.
```
center = node.aabb.center()
lowPoint  = origin + lT*dir
highPoint = origin + hT*dir

// Which axes' mid-plane does the [lT,hT] segment actually cross?
comps = []                       // axis indices, ≤3
for i in 0..3:
    // exact C++ (CAreaOctTree.cpp:317-321): skip axis i when
    //   (lo[i] >= c[i] || hi[i] <= c[i]) && (hi[i] >= c[i] || lo[i] <= c[i])
    // i.e. keep it only when the [lT,hT] segment strictly straddles the mid-plane c[i].
    if (lo[i] >= c[i] || hi[i] <= c[i]) && (hi[i] >= c[i] || lo[i] <= c[i]): continue
    if |dir[i]| <= 1e-4: continue
    compT[i] = dirRecip[i] * (center[i] - origin[i])   // ray param where it crosses mid-plane i
    comps.push(i)

sort comps ascending by compT[axis]     // 0,1,2,3 entries; explicit sort in C++

// starting octant = octant of the segment's start point
start = origin + lT*dir
selector = (start.x >= center.x) | (start.y >= center.y)<<1 | (start.z >= center.z)<<2

tLo = lT
for k in -1 .. comps.len()-1:            // one segment per crossed plane, plus the tail
    if k >= 0: selector ^= 1 << comps[k]        // flip the axis bit we just crossed
    tHi = (k < comps.len()-1) ? compT[comps[k+1]] : hT
    if tHi > lowT && tLo <= tHi:
        child = node.GetChild(selector)
        if child.type != Invalid:
            child.LineTestExInternal(line, filter, res, tLo, tHi, maxT, dirRecip)
        if res.surface:                        // first hit wins — front-to-back guarantees nearest
            if res.t > highT: res = invalid    // ...unless it's actually past this node
            break
    tLo = tHi
```

**Branch case, `childFlags == 0x0A` ("two leaves"):**
```
for i in 0..2:
    child = GetChild(i)                        // leaf, stored AABB
    tf1, tf2 = lT, hT
    if BoxLineTest(child.aabb, line, tf1, tf2):
        child.LineTestExInternal(line, filter, tmp[i], tf1, tf2, maxT, dirRecip)
res = nearer of tmp[0]/tmp[1] (by t), or invalid if neither hit
if res.t > highT: res = invalid
```

**Why this matters / why you can cheat:** the descent is strictly front-to-back
and each recursion is clamped to a sub-interval of `[0, maxT]`, and it `break`s on
the first surface found (after a `res.t <= highT` sanity check). Therefore the
returned surface is exactly *the nearest filter-passing triangle along the ray
within `[0, length]`.* A brute-force loop over the master triangle list, running
the same triangle test and keeping the min `t`, produces the **same hit** — see §7.

--------------------------------------------------------------------------------
## 5. Triangle test & surface reconstruction

### 5.1 Möller–Trumbore, as inlined in the octree  (`CAreaOctTree.cpp:108-149 / 225-271`)

**This is two-sided** (accepts negative determinant → back-faces hit too), unlike
`CollisionUtil::RayTriangleIntersection` which is one-sided. Use this version.

```
E = FLT_EPSILON                    // ~1.1920929e-7
e0 = v1 - v0
e1 = v2 - v0
P  = dir × e1
det = P · e0
if |det| < E*10:  continue         // ray ∥ triangle
invDet = 1/det
T = origin - v0
u = invDet * (T · P);              if u < 0 || u > 1:        continue
Q = T × e0
t = invDet * (Q · e1);            if t >= bestT || t < lowT: continue    // bestT starts = highT
v = invDet * (Q · dir);          if v < 0 || u + v > 1:     continue
matWord = materials[polyMats[triIdx]]
if filter.Passes(matWord) && t <= bestT:
    bestT = t; hit = { surface, t }
```

`bestT` is per-leaf, initialised to the incoming `highT`; the leaf returns its
closest hit.

### 5.2 Master-list triangle reconstruction  (`CAreaOctTree.cpp:591-604`, `GetMasterListTriangle`)

Given triangle index `idx` (0..polyCount):
```
e0 = edges[polyEdges[idx*3 + 0]]        // {vi1, vi2}
e1 = edges[polyEdges[idx*3 + 1]]
vert2 = e1.vi2
if e1.vi1 != e0.vi1 && e1.vi1 != e0.vi2:
    vert2 = e1.vi1
matWord = materials[polyMats[idx]]
if matWord & 0x0200_0000:               // bit 25 "RedundantEdge / FlippedTri"
    surface verts = ( verts[e0.vi2], verts[e0.vi1], verts[vert2] )
else:
    surface verts = ( verts[e0.vi1], verts[e0.vi2], verts[vert2] )
```
Note: only the **first two** edges of the triangle are used to pick the 3 verts.
(This differs from `collision_mesh.rs::build_vertices`, which does a 3-edge walk
for rendering — for the ray tracer, replicate `GetMasterListTriangle` exactly.)

`verts[i]` = `(verts_f32[i*3], verts_f32[i*3+1], verts_f32[i*3+2])`.

### 5.3 `CCollisionSurface` normal & plane  (`CCollisionSurface.cpp:10-18`)

```
normal = normalize( (v1 - v0) × (v2 - v0) )      // right-handed; winding set by 5.2
plane  = CPlane(normal, normal · v0)             // i.e. plane.d = normal · v0
```
This normal/plane is what ends up in `CRayCastResult.plane`. The final result's
`material` is the raw `matWord` (§5.2), wrapped as a 64-bit `CMaterialList` (low
32 bits = surface word).

--------------------------------------------------------------------------------
## 6. Combine static + dynamic  (`CGameCollision.cpp:347-362`)

```
staticRes  = RayStaticIntersection(...)
dynamicRes = RayDynamicIntersection(..., idOut, ...)   // writes idOut on hit
if dynamicRes.IsValid():
    if staticRes.IsInvalid():           return dynamicRes
    if staticRes.t >= dynamicRes.t:     return dynamicRes
return staticRes
```
So: static wins ties and wins when strictly nearer; `idOut` is only meaningful
when the dynamic result is returned.

--------------------------------------------------------------------------------
## 7. Recommended implementation for `primewatch3`

**Primary: brute-force the master triangle list.** For each area's
`CollisionMesh` (already parsed):

```
best_t = length                       // (or +inf if length <= 0)
best   = None
for idx in 0..polyCount:
    (v0,v1,v2), mat = master_list_triangle(idx)      // §5.2
    if !filter_passes(mat): continue                 // §5 / §9 ; default filter = accept all
    if let Some(t) = moller_trumbore_two_sided(pos, dir, v0, v1, v2, lo=0.0, hi=best_t):   // §5.1
        best_t = t
        best   = Some(RayHit { t, point: pos + dir*t, normal, plane_d: normal·v0, mat, area, tri: idx })
```

This is **behaviourally identical** to `RayStaticIntersection` for any ray that
isn't grazing a node boundary at almost exactly the hit distance (the only place
the octree's `1±1.2e-5` fudge factors on per-node `lowT/highT` can change the
answer). For a live-inspection overlay that tolerance is irrelevant, and it drops
the entire tree-format parser.

Keep `dir` normalised and `t` therefore in world units. Match the game epsilons
(`E*10` for the determinant reject, `t >= hi || t < lo`).

**Only if bit-exact octree behaviour is needed** (e.g. reproducing a specific
physics desync): implement §4.2 + §4.4. The node parser is ~120 lines; the
`.bs` schema cannot describe the variable-length node bytes, so it must be a
hand-written reader over `GameMemory` starting at `CAreaOctTree.treebuff`.

**Dynamic actors (§8): defer.** Static geometry is "what the game's world
collision does"; add dynamic later behind the same `RayHit` return type.

--------------------------------------------------------------------------------
## 7a. Performance — is brute force fast enough?

### Measured / estimated

An area's `CAreaOctTree` holds ~3k–30k triangles (big caverns ~40k; the
`load_mesh` sanity cap is 50k). The one area resident in `mem1.raw` is
**9,208 tris**.

Scalar two-sided Möller–Trumbore in **release** Rust, with the early-outs, costs
~5–15 ns per triangle:

| tris | 1 ray | 10 rays | 200 rays |
|------|-------|---------|----------|
| 9k   | ~90 µs | ~0.9 ms | ~18 ms |
| 30k  | ~300 µs | ~3 ms | ~60 ms |

(Debug builds run 10–20× slower — never judge this in a debug build.)
Per-frame budget at 60 fps is 16.6 ms, shared with the memory snapshot + egui +
wgpu render, so keep ray work under ~2 ms.

### It comes down to how many rays per frame

* **Replicating the game literally** — `CGameCollision::MoveAndCollide` issues
  **one** `RayWorldIntersection` per moving physics actor per frame, and only on
  the `transMag > halfExtent` fast path. Simulating the player + a handful of
  actors ⇒ ~1–10 rays/frame ⇒ **brute force is comfortably fine** (< 1 ms).
  Start here — it is ~40 lines and needs no tree parser.
* **Dense uses** — a debug ray-fan, live mouse-pick with continuous feedback, or
  multi-frame look-ahead (re-simulating N frames every displayed frame) — can
  reach hundreds–thousands of rays and blow the budget.

### If you need dense: build your *own* BVH, don't reuse the game octree

Priority order:

1. **Brute force** over `CollisionMesh` (already parsed). Simplest, correct,
   covers the literal-sim case.
2. **Brute force + SIMD + `rayon`.** The tri loop vectorises 4–8 wide trivially
   and rays are embarrassingly parallel. ~200 rays × 9k tris / 8 lanes / 8 cores
   ≈ ~1 ms with *no* acceleration structure. This is the pragmatic sweet spot for
   the "more than a few, fewer than thousands" range.
3. **A BVH built from the triangle soup**, once per area load, **cached**
   (collision geometry is static — never rebuild). ~1 ms build over 10k tris;
   query is ~log(n). Use a crate (`bvh`, `parry3d`) or ~150 lines.

**Do not** walk the game's octree bytes live as the acceleration structure, and
**do not** re-pack them into your own tree:

* The node blob is big-endian, variable-length, and offset-pointer-chained
  (§4.2). Every node touch during traversal goes through `GameMemory`
  (mask + bounds-check + byte-swap per field) — slow and fragile.
* A freshly built BVH out-performs a faithful port of `LineTestExInternal` *and*
  is far less code.
* Re-packing the game octree buys nothing: its only advantage is bit-exact
  traversal order, which is lost the moment you repack — and which never changes
  the hit anyway (the `1 ± 1.2e-5` per-node `lowT/highT` fudge shifts results by
  sub-micron distances, never a different triangle, never a physics outcome).

Reserve a literal port of the game's §4.2/§4.4 traversal for the one scenario
where you must reproduce a specific physics divergence down to the ULP — and even
then it is almost certainly not the octree epsilons that cause it.

--------------------------------------------------------------------------------
## 8. Dynamic half — `RayDynamicIntersection`  (`CGameCollision.cpp:295-320`)

```
bestT = length (or 100000 if <= 0)
for id in nearList:
    actor = GetObjectById(id) as CPhysicsActor?      // skip non-physics
    xf   = actor.GetPrimitiveTransform()             // primitive transform (translation + orientation)
    prim = actor.GetCollisionPrimitive()
    res  = prim->CastRay(pos, dir, bestT, filter, xf)
    if res.IsValid() && res.t < bestT:
        bestT = res.t; result = res; idOut = id
```

`nearList` is built by `CStateManager::BuildColliderList` from the actor's motion
volume — for a tool, "all physics actors whose bounds are near the ray" is a fine
approximation, or skip it entirely.

`prim->CastRay(start, dir, length, filter, xf)` per primitive type:

* **Sphere (`SPHR`)** — `CCollidableSphere::CastRayInternal` (`CCollidableSphere.cpp:208`):
  transform sphere by `xf` (position `xf * localPos`, radius unchanged), then
  `CollisionUtil::RaySphereIntersection`:
  ```
  rayToSphere = sphere.pos - start
  magSq = |rayToSphere|²;  dirDot = rayToSphere · dir;  radSq = r²
  if dirDot < 0 && magSq > radSq: miss                       // pointing away, outside
  isq = radSq - (magSq - dirDot²);  if isq < 0: miss
  t = (magSq > radSq) ? dirDot - sqrt(isq)                   // outside → near root
                      : dirDot + sqrt(isq)                   // inside  → far root
  if t < length || length == 0:  hit at start + t*dir
  ```
  plane normal = `(point - sphere.pos)` normalised (or `dir` if within 0.01),
  `plane.d = point · normal`.

* **AABox (`AABX`)** — `CCollidableAABox::CastRayInternal` (`CCollidableAABox.cpp:28`):
  transform ray into box-local space (`xf.Inverse()`), run
  `CollisionUtil::BoxLineTest` (slab test, returns `tMin,tMax,axis,sign`), reject
  if `tMin < 0` or `tMin > length`. Hit plane normal = `±unit axis` (from `sign`),
  result then transformed back to world by `xf`.

* **OBBTreeGroup (`OBTG`)** — `CCollidableOBBTree`; a mini-octree over an OBB tree.
  Rare for ray casts; skip unless needed.

`CCollisionPrimitive.x8_material` is filter-tested up front (`filter.Passes(prim
material)`) — a primitive whose material fails the filter is skipped entirely.

--------------------------------------------------------------------------------
## 9. Material filter — `CMaterialFilter`  (`CMaterialFilter.hpp`)

```
CMaterialList include   // u64, default 0x0000_0000_FFFF_FFFF
CMaterialList exclude   // u64, default 0
EFilterType   type      // Always | Include | Exclude | IncludeExclude

Passes(listWord):
    Always         -> true
    Include        -> (listWord & include) != 0
    Exclude        -> (listWord & exclude) == 0
    IncludeExclude -> (listWord & include) != 0  &&  (listWord & exclude) == 0
```

* Triangle surface words only ever set bits 0..31; the bit layout **matches**
  `prime_defs` `enum CollisionMaterial` (bit 31 = FLOOR, 30 = WALL, 29 = CEILING,
  19 = SOLID, 25 = REDUNDANT_EDGE/FLIPPED_TRI, 18 = SHOOT_THRU, 27 = SCAN_THRU,
  26 = SEE_THRU, …). `EMaterialTypes` bits ≥ 32 (Player, Character, Trigger…) are
  only relevant on actors, not static tris.
* `skPassEverything` / a default-constructed filter → `Always` → every triangle
  passes. That is the right default for a generic "what would a ray hit" overlay.
* The game's actual player-motion cast passes `actor.GetMaterialFilter()`. For the
  morph ball / player that is typically an `Exclude` of the passthrough materials
  (projectile / scan / camera / see-through / AI). Expose this as a config toggle
  in the tool; start with pass-everything.

--------------------------------------------------------------------------------
## 10. Schema (`prime_defs`) status

**Static path — everything needed is already present:**

| def | file | status |
|-----|------|--------|
| `CAreaOctTree` (aabb, treeType, treebuff, all master arrays) | `CAreaOctTree.bs` | ✅ present |
| `CCollisionEdge` `{u16 edge1, u16 edge2}` | `CAreaOctTree.bs` | ✅ present |
| `enum CollisionMaterial : u32` | `CAreaOctTree.bs` | ✅ present |
| `CGameArea` → `postConstructed` → `collision` walk | `CGameArea.bs` / `CPostConstructed.bs` | ✅ present (used by `load_mesh`) |
| `CAABB`, `CVector3f`, `CTransform` | `math/` | ✅ present |

The tree-node bytes (§4.2) are a variable-length hand-rolled format and
**cannot** be a `.bs` struct — they get a hand-written reader (only needed if we
do the exact octree traversal rather than the brute-force scan).

**Dynamic path — partial, needs work if/when we do §8:**

| need | current state |
|------|---------------|
| `CPhysicsActor.collisionPrimitive` as a *polymorphic* primitive (sphere vs AABox vs OBB-tree) | `CPhysicsActor.bs` models it as an inline `CAABBPrimitive` at `0x1C0` (AABox-only). Fine for most actors, wrong for the morph ball (sphere) and anything using an OBB tree. |
| `CCollidableSphere` layout (`x10_sphere : CSphere`) | not in `prime_defs`; `math/CSpherePrimitive.bs` has `CSphere {origin, radius}` but no `CCollidableSphere` wrapper |
| `CCollidableAABox` layout (`x10_aabox : CAABox`, material at `x8`) | not in `prime_defs` |
| `CCollisionPrimitive` base (`x8_material : CMaterialList`) + how to tell the runtime type from memory (vtable ptr → RTTI/type table) | not in `prime_defs` |
| `CMaterialFilter` / `CMaterialList` as readable structs (to pull `actor.GetMaterialFilter()` from memory) | not in `prime_defs` — only needed if we want the *actor's* real filter rather than a tool-chosen one |

### Requests

If we want the **dynamic** half at real fidelity, the following need adding to
`../primewatch2/prime_defs/` (authoritative) and copying here:

1. `CCollisionPrimitive` base — offset of `x8_material` (`CMaterialList`, u64) and
   the vtable/type-table pointer, plus a note on how `GetPrimType()` /
   `GetTableIndex()` map to `SPHR` / `AABX` / `OBTG`.
2. `CCollidableSphere` — offset of the `CSphere` member (`x10_sphere`).
3. `CCollidableAABox` — offset of the `CAABox` member (`x10_aabox`).
4. Whether `CPhysicsActor` `0x1C0` is really an inline object or a pointer to a
   heap primitive, and the real primitive-transform accessor
   (`GetPrimitiveTransform` = `transform.rotation` + `translation + primitiveOffset`?).
5. (optional) `CMaterialFilter` (`include:u64 @0x0`, `exclude:u64 @0x8`,
   `type:u32 @0x10`) if we want to read an actor's own filter.

The **static** ray tracer can be built right now with zero schema changes.
