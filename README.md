# OpenSDTS

**Open Software Defined Target System**

> An open platform for rendering, tracking, and scoring virtual targets
> independently of the underlying hardware.

## Vision

OpenSDTS separates **what a target is** from **how it is displayed** and
**how impacts are measured**.

``` text
Scenario
    │
    ▼
 SDTS Core
 ├── Renderer
 ├── Impact Adapter
 └── Scoring Engine
```

The long-term goal is to support multiple renderers (laser, monitor,
projector), multiple impact detectors, and multiple target scenarios
through a common architecture.

## MVP

The first milestone is intentionally small:

-   Browser renderer
-   Moving circle target
-   Mouse clicks as impacts
-   Hit/miss scoring
-   JSON protocol over WebSocket
-   Session recording
-   Declarative target paths and timed sequences
-   Seeded random positions and delays
-   Browser scenario selection

No laser hardware is required.

## Repository Layout

``` text
docs/
src/
examples/
recordings/
```

See [docs/index.md](docs/index.md) for the full documentation set.
