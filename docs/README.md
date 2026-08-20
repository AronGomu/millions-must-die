# Design Documentation

1. [Context](CONTEXT.md) — vision, roadmap, MVP scope
2. [Design](DESIGN.md) — technical architecture, design decisions
3. [Glossary](GLOSSARY.md) — shared vocabulary
4. [Testing strategy](05-testing.md)

## Phase 0 architecture

- [Technical prototype](technical-prototype-architecture.html)
- [Simulation + navigation](simulation-navigation-architecture.html)
- [Agent collision](agent-collision-architecture.html)
- [Sprite renderer](sprite-renderer-architecture.html)
- [Local validation lab](local-validation-lab-architecture.html)
- [Architecture decision records](ADR/README.md)

## Phase 0.5 architecture

- [Horde sim headroom](horde-sim-headroom-architecture.html)

## Phase 1 architecture

- [RTS engine prototype](rts-engine-prototype-architecture.html) — two entity
  models in one binary, the texture table, the render layers
- [RTS engine prototype functional close](rts-engine-prototype-functional-close.md)
  — what phase 1 proves, what it does not, and every known gap
- [Architecture decision records](ADR/README.md) — 013, 014, 015

## Phase 1.1 architecture

- [RTS interaction, UI + audio hardening](rts-interaction-ui-audio-hardening-architecture.html)
  — the implemented pick/body/nav/settings/window/HUD/minimap/audio
  architecture, with the files and tests that back each part
- [RTS interaction, UI + audio hardening functional close](rts-interaction-ui-audio-hardening-functional-close.md)
  — what phase 1.1 proves, what no offscreen test can claim, and every known gap
- [Architecture decision records](ADR/README.md) — 016–020
- Manual (human-only) checks: `../artifacts/manual_test_checklist.md`

## Feedback polish

- [RTS feedback polish](rts-feedback-polish-architecture.html) — the landed
  control/settings/grid/placement/pick/command/collision design, with the test
  that backs each claim
- [RTS feedback polish functional close](rts-feedback-polish-functional-close.md)
  — what it proves, what only a human can check, the narrowed collision
  invariant, and the one open regression
- [Architecture decision records](ADR/README.md) — 021

## Implementation plan

- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.html`
