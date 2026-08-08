#!/usr/bin/env python3
"""Generate the tracked collision demo scenes and their sha256 sidecars.

Deterministic and idempotent: rerunning it must leave `git status` clean.
The scenes are tracked assets, like the gate scene and the fixtures; this
script exists for provenance, not as a build step.

Usage (from the workspace root):
    python3 tools/scenegen/gen_collision_scenes.py
"""

import hashlib
import pathlib

WIDTH, HEIGHT = 480, 270
CELL_PX, SPRITE_PX = 4, 30
DEST = (240, 135)

SCENES = [
    # (filename, hard_agents, collision_radius_q8, seed)
    ("collision_mid_v1.ron", 10_000, 320, 7355608251463129073),
    ("collision_sprite_v1.ron", 1_200, 960, 4411920776318552601),
]


def obstacle_cells():
    cells = set()
    for by in range(0, HEIGHT, 18):
        for bx in range(0, WIDTH, 24):
            for dy in range(4):
                for dx in range(4):
                    x, y = bx + 12 + dx, by + 9 + dy
                    if x < WIDTH and y < HEIGHT:
                        cells.add(x + y * WIDTH)
    return sorted(cells)


def spawn_cells():
    return [(2, y) for y in range(8, 264, 2)]


def render(name, hard, radius_q8, seed, obstacles, spawns):
    lines = [
        "(",
        f'  version: "collision_scene_v1",',
        f"  width: {WIDTH},",
        f"  height: {HEIGHT},",
        f"  cell_size_px: {CELL_PX},",
        f"  sprite_size_px: {SPRITE_PX},",
        f"  hard_agent_count: {hard},",
        "  stretch_agent_count: 20000,",
        f"  seed: {seed},",
        f"  destination: (x: {DEST[0]}, y: {DEST[1]}),",
        "  spawn_cells: [",
    ]
    lines += [f"    (x: {x}, y: {y})," for x, y in spawns]
    lines += [
        "  ],",
        "  atlas_count: 4,",
        "  direction_count: 8,",
        "  frame_count: 4,",
        f"  collision_radius_q8: {radius_q8},",
        "  separation_strength_q8: 256,",
        "  obstacle_cells: [",
    ]
    for i in range(0, len(obstacles), 20):
        lines.append("    " + ",".join(str(c) for c in obstacles[i : i + 20]) + ",")
    lines += ["  ],", ")", ""]
    return "\n".join(lines)


def main():
    out_dir = pathlib.Path("assets/scenarios")
    assert out_dir.is_dir(), "run from the workspace root"
    obstacles = obstacle_cells()
    spawns = spawn_cells()
    dest_idx = DEST[0] + DEST[1] * WIDTH
    assert dest_idx not in set(obstacles), "destination must be free"
    blocked = set(obstacles)
    for x, y in spawns:
        assert x + y * WIDTH not in blocked, f"spawn ({x}, {y}) must be free"
    for name, hard, radius_q8, seed in SCENES:
        text = render(name, hard, radius_q8, seed, obstacles, spawns)
        path = out_dir / name
        path.write_text(text)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        path.with_suffix(".sha256").write_text(digest + "\n")
        print(f"{path}: agents={hard} radius_q8={radius_q8} sha256={digest}")


if __name__ == "__main__":
    main()
