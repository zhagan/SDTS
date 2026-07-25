# Declarative scenarios

Scenario definitions live in `scenarios/*.json`. The Core loads a selected
definition when a live session starts; the browser lists valid definitions
from `GET /api/scenarios`.

Each scenario defines an arena and one or more targets. Every target has its
own display settings and sequence, so sequences can run independently.

```json
{
  "name": "Random Pop-up",
  "description": "A timed random-position drill.",
  "seed": 42,
  "arena": { "width_mm": 1600, "height_mm": 1000 },
  "targets": [
    {
      "id": "popup",
      "display": {
        "shape": "circle",
        "radius_mm": 75,
        "fill": "#ef8354",
        "stroke": "#d9683c"
      },
      "repeat": true,
      "sequence": [
        {
          "action": "show",
          "duration_secs": 3,
          "position": {
            "type": "random",
            "margin_mm": 25,
            "minimum_distance_mm": 300
          }
        },
        {
          "action": "hide",
          "duration_secs": {
            "random": { "min": 1, "max": 3 }
          }
        }
      ]
    }
  ]
}
```

## Steps

A sequence contains `show` and `hide` steps. Durations can be a fixed number
of seconds or a random range:

```json
"duration_secs": 3
```

```json
"duration_secs": { "random": { "min": 1, "max": 3 } }
```

A show step accepts either a position or path. Supported positions are
`fixed` and `random`. Supported paths are `line`, `polyline`, `zigzag`, and
`random_line`; see the checked-in definitions for complete examples.

`random_line` chooses a starting position and travel direction using the
scenario seed:

```json
"path": {
  "type": "random_line",
  "margin_mm": 25,
  "distance_mm": { "min": 350, "max": 900 }
}
```

The show step's `duration_secs` controls the movement speed. Increasing the
duration makes the same randomly selected travel distance take longer.

Random positions automatically keep the complete target inside the arena.
`margin_mm` adds space beyond the target radius, and
`minimum_distance_mm` asks the evaluator to choose a position away from the
previous appearance.

## Determinism

Random durations and positions are derived from `seed`, target ID, cycle,
and sequence-step index. Loading the same definition with the same seed
produces the same sequence. The Core still records emitted updates, so replay
uses the original events rather than recalculating the scenario.

## Running scenarios

Choose a definition in the browser and press **Start Scenario**, or pass a
definition filename when starting the Core:

```text
cargo run -- random-popup.json
```

Definitions are validated at load time. Filenames must be bare `.json`
filenames inside the `scenarios` directory.
