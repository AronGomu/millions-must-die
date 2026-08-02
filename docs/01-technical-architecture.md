# Technology Architecture

- Language: Rust
- Platform: SDL3
- Engine: Custom
- Rendering: Batched GPU sprite renderer
- Simulation: Data-oriented
- Navigation: Flow fields
- Spatial partition: Uniform grid (post-phase-0; deferred until a gameplay system consumes it)
- Native first, browser/WebAssembly later.

Rules:
- No per-frame allocations in simulation.
- No per-enemy pathfinding.
- Benchmark every major system.

Detailed phase-0 designs:
- [Technical prototype](technical-prototype-architecture.html)
- [Simulation and navigation](simulation-navigation-architecture.html)
- [Sprite renderer](sprite-renderer-architecture.html)
- [Local validation lab](local-validation-lab-architecture.html)
- [Architecture decision records](ADR/README.md)
