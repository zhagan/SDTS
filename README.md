# OpenSDTS

**Open Software Defined Target System**

OpenSDTS is a platform for creating, rendering, and scoring dynamic target scenarios.

Define a scenario once, run it anywhere.

Today that means a browser with mouse input. Tomorrow it could be a projector, laser system, electronic target, or any other renderer and impact detector.

---

## What is it?

A scenario is described declaratively.

```text
Load Scenario
      │
      ▼
  SDTS Core
      │
      ├── Renderer
      ├── Impact Adapter
      ├── Scoring
      └── Recorder
```

The Core owns the authoritative state of every target.

Renderers display targets.

Impact adapters report impacts.

The scoring engine evaluates hits.

The recorder captures every event for deterministic replay.

---

## Current Features

* Browser renderer
* Moving targets
* Hit / miss scoring
* Session replay
* Declarative JSON scenarios
* Timed sequences
* Deterministic randomization
* Multiple target path types
* Browser scenario selection
* JSON protocol over WebSocket

---

## Example Scenario

```json
{
  "name": "Random Pop-up",
  "seed": 42,
  "targets": [
    {
      "display": {
        "shape": "circle",
        "radius_mm": 75
      },
      "repeat": true,
      "sequence": [
        {
          "action": "show",
          "duration_secs": 3,
          "position": {
            "type": "random"
          }
        },
        {
          "action": "hide",
          "duration_secs": {
            "random": {
              "min": 1,
              "max": 3
            }
          }
        }
      ]
    }
  ]
}
```

---

## Design Goals

* Hardware independent
* Deterministic execution
* Replayable sessions
* Simple protocol
* Declarative scenarios
* Modular architecture
* Open source

---

## Long-Term Vision

OpenSDTS separates **what a target does** from **how it is displayed** and **how impacts are measured**.

This allows the same scenario to run against different combinations of:

* Browser renderer
* Video projector
* Laser projector
* Electronic target systems
* Future renderers

and

* Mouse input
* Electronic impact detectors
* Future impact adapters

without changing the scenario definition.

---

## Repository Layout

```text
docs/
examples/
recordings/
scenarios/
src/
```

---

## Documentation

* Architecture
* Components
* Scenario Format
* SDTP Protocol
* MVP
* Roadmap

---

## Status

OpenSDTS is currently focused on building a robust reference implementation using a browser renderer.

Future work will add additional renderers, impact adapters, and more sophisticated target scenarios while keeping the Core and scenario format unchanged.
