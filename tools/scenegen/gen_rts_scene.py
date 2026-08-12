#!/usr/bin/env python3
"""Generate the tracked RTS prototype scene and its sha256 sidecar.

Deterministic and idempotent: rerunning it must leave `git status` clean.
The scene is a tracked asset, like the gate scene, the collision demo scenes
and the fixtures; this script exists for provenance, not as a build step.

Usage (from the workspace root):
    python3 tools/scenegen/gen_rts_scene.py
"""

import hashlib
import pathlib

VERSION = "rts_prototype_v1"
WIDTH, HEIGHT = 320, 320
CELL_PX, SPRITE_PX = 4, 48
SEED = 1743218095512340987
DEST = (165, 176)
SPAWNS = [(x, 178) for x in range(162, 168)]

# `scenario::HQ_FOOTPRINT_CELLS`. Kept in step by hand: this file is
# provenance, not a build step, and importing a Rust constant into it would
# be a build step.
HQ_FOOTPRINT_CELLS = 12
HQ_CELL = (160, 160)
HQ_CENTER = (165, 165)

START_CRYSTAL = 300
START_GAS = 100
START_SUPPLY_CAP = 10

CRYSTAL_NODES = [
    (140, 150),
    (146, 146),
    (152, 142),
    (180, 142),
    (186, 146),
    (192, 150),
    (150, 190),
    (182, 190),
]
GAS_NODES = [(136, 168), (196, 168)]

HARD_AGENT_COUNT = 0

# `scenario::MAX_LIVE_AGENTS`. This family is horde-free by construction, so
# it declares 0 regardless, but the loader still refuses anything above the
# engine ceiling.
MAX_LIVE_AGENTS = 5_000


def chebyshev(a, b):
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]))


def obstacle_cells():
    all_nodes = CRYSTAL_NODES + GAS_NODES
    spawn_set = set(SPAWNS)
    cells = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if (x * 7 + y * 13) % 97 != 0:
                continue
            if chebyshev((x, y), HQ_CENTER) <= 28:
                continue
            if any(chebyshev((x, y), n) <= 6 for n in all_nodes):
                continue
            if (x, y) in spawn_set:
                continue
            if (x, y) == DEST:
                continue
            cells.append(x + y * WIDTH)
    return sorted(cells)


def bfs_reachable_8(origin, blocked, width, height):
    """8-neighbour BFS over unblocked cells, for the generator's own sanity
    checks. The loader's own reachability rule is 4-connected; this stronger
    (superset-connectivity) precondition is what the generator asserts before
    it ever writes a file for the loader to validate."""
    seen = {origin}
    stack = [origin]
    while stack:
        x, y = stack.pop()
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dx == 0 and dy == 0:
                    continue
                nx, ny = x + dx, y + dy
                if not (0 <= nx < width and 0 <= ny < height):
                    continue
                if (nx, ny) in seen:
                    continue
                if (nx + ny * width) in blocked:
                    continue
                seen.add((nx, ny))
                stack.append((nx, ny))
    return seen


def render(obstacles):
    lines = [
        "(",
        f'  version: "{VERSION}",',
        f"  width: {WIDTH},",
        f"  height: {HEIGHT},",
        f"  cell_size_px: {CELL_PX},",
        f"  sprite_size_px: {SPRITE_PX},",
        f"  hard_agent_count: {HARD_AGENT_COUNT},",
        f"  stretch_agent_count: {HARD_AGENT_COUNT},",
        f"  seed: {SEED},",
        f"  destination: (x: {DEST[0]}, y: {DEST[1]}),",
        "  spawn_cells: [",
    ]
    lines += [f"    (x: {x}, y: {y})," for x, y in SPAWNS]
    lines += [
        "  ],",
        "  atlas_count: 4,",
        "  direction_count: 8,",
        "  frame_count: 4,",
        "  collision_radius_q8: 768,",
        "  separation_strength_q8: 256,",
        "  separation_phases: 1,",
        "  mass_class_count: 1,",
        "  separation_threads: 1,",
        "  obstacle_cells: [",
    ]
    for i in range(0, len(obstacles), 20):
        lines.append("    " + ",".join(str(c) for c in obstacles[i : i + 20]) + ",")
    lines += [
        "  ],",
        "  rts: Some((",
        f"    start_crystal: {START_CRYSTAL},",
        f"    start_gas: {START_GAS},",
        f"    start_supply_cap: {START_SUPPLY_CAP},",
        f"    hq_cell: (x: {HQ_CELL[0]}, y: {HQ_CELL[1]}),",
        "    crystal_nodes: [",
    ]
    lines += [f"      (x: {x}, y: {y})," for x, y in CRYSTAL_NODES]
    lines += [
        "    ],",
        "    gas_nodes: [",
    ]
    lines += [f"      (x: {x}, y: {y})," for x, y in GAS_NODES]
    lines += [
        "    ],",
        "  )),",
        ")",
        "",
    ]
    return "\n".join(lines)


def main():
    out_dir = pathlib.Path("assets/scenarios")
    assert out_dir.is_dir(), "run from the workspace root"

    obstacles = obstacle_cells()
    blocked = set(obstacles)

    assert HARD_AGENT_COUNT == 0, "the loader refuses a nonzero population for this family"
    assert HARD_AGENT_COUNT <= MAX_LIVE_AGENTS

    dest_idx = DEST[0] + DEST[1] * WIDTH
    assert dest_idx not in blocked, "destination must be free"

    for x, y in SPAWNS:
        assert (x + y * WIDTH) not in blocked, f"spawn ({x}, {y}) must be free"

    reach = bfs_reachable_8(DEST, blocked, WIDTH, HEIGHT)
    for x, y in SPAWNS:
        assert (x, y) in reach, f"spawn ({x}, {y}) must be reachable from the destination"

    hq_x0, hq_y0 = HQ_CELL
    hq_cells = [
        (hq_x0 + dx, hq_y0 + dy)
        for dy in range(HQ_FOOTPRINT_CELLS)
        for dx in range(HQ_FOOTPRINT_CELLS)
    ]
    for x, y in hq_cells:
        assert 0 <= x < WIDTH and 0 <= y < HEIGHT, f"hq footprint cell ({x}, {y}) out of bounds"
        assert (x + y * WIDTH) not in blocked, f"hq footprint cell ({x}, {y}) must be free"

    hq_set = set(hq_cells)
    all_nodes = CRYSTAL_NODES + GAS_NODES
    assert len(set(all_nodes)) == len(all_nodes), "node cells must not repeat across kinds"
    for x, y in all_nodes:
        assert (x + y * WIDTH) not in blocked, f"node ({x}, {y}) must be free"
        assert (x, y) in reach, f"node ({x}, {y}) must be reachable from the destination"
        assert (x, y) not in hq_set, f"node ({x}, {y}) must not lie inside the HQ footprint"

    for x, y in SPAWNS:
        assert (x, y) not in hq_set, f"spawn ({x}, {y}) must not lie inside the HQ footprint"

    text = render(obstacles)
    path = out_dir / f"{VERSION}.ron"
    path.write_text(text)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    path.with_suffix(".sha256").write_text(digest + "\n")
    print(
        f"{path}: obstacles={len(obstacles)} "
        f"crystal_nodes={len(CRYSTAL_NODES)} gas_nodes={len(GAS_NODES)} sha256={digest}"
    )


if __name__ == "__main__":
    main()
