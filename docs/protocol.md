# SDTP Summary

Envelope:

``` json
{
  "type":"impact",
  "time":1.23,
  "source":"mouse",
  "data":{}
}
```

Rules

-   JSON over WebSocket
-   Millimeters
-   Seconds from session start
-   Immutable events
-   Unknown fields ignored

`session_start.data.scenario` contains the selected scenario filename for a
live session. Older recordings may omit it.

## Impact results

The mouse impact adapter sends the click position in arena coordinates:

```json
{
  "type": "impact",
  "time": 1.23,
  "source": "mouse",
  "data": {
    "impact_id": "7c488bfd-0b66-4b95-9b83-a352ea10c816",
    "x_mm": 320.5,
    "y_mm": 184.0
  }
}
```

After scoring the impact against the authoritative target position, the Core
emits a result:

```json
{
  "type": "result",
  "time": 1.24,
  "source": "core",
  "data": {
    "target_id": "circle-1",
    "impact_id": "7c488bfd-0b66-4b95-9b83-a352ea10c816",
    "hit": true,
    "distance_mm": 12.7,
    "x_mm": 320.5,
    "y_mm": 184.0,
    "hits_remaining": null
  }
}
```

For a `result`, `x_mm` and `y_mm` are the impact coordinates, not the target
center. Renderers use these coordinates to draw feedback at the location of
the click. `distance_mm` is the distance between that impact and the target's
authoritative center at scoring time. `hits_remaining` is `null` unless the
target has a `durability` budget (see below), in which case it's the number
of hits left before the target is destroyed — `0` on the hit that destroyed
it.

## Target display and visibility

`target_spawn` describes how a renderer should draw a target:

```json
{
  "type": "target_spawn",
  "time": 0,
  "source": "core",
  "data": {
    "target_id": "popup",
    "shape": "circle",
    "radius_mm": 75,
    "fill": "#ef8354",
    "stroke": "#d9683c"
  }
}
```

`target_update` carries position, visibility, and the target's current
radius. A hidden target must not be rendered or considered by scoring.

```json
{
  "type": "target_update",
  "time": 3,
  "source": "core",
  "data": {
    "target_id": "popup",
    "x_mm": 320.5,
    "y_mm": 184,
    "visible": false,
    "radius_mm": 75
  }
}
```

`radius_mm` here is the target's *live* radius, which only ever differs from
the constant one in its `target_spawn` for a target with a `durability`
budget (below) that has taken damage — it shrinks per hit and resets to the
base radius the instant a fresh appearance begins.

## Destructible targets

A target definition can include an optional `durability` block making it
destructible:

```json
{
  "id": "gallery-1",
  "display": { "shape": "circle", "radius_mm": 90, "fill": "#c77dff", "stroke": "#9d4edd" },
  "repeat": true,
  "durability": {
    "hits_to_destroy": 3,
    "shrink_per_hit": 0.75,
    "min_radius_factor": 0.4
  },
  "sequence": [ ... ]
}
```

- `hits_to_destroy` — number of hits this appearance can take before it's
  destroyed.
- `shrink_per_hit` — radius multiplier applied after each non-destroying
  hit (default `1.0`, i.e. no shrink — the target just takes
  `hits_to_destroy` hits at a constant size).
- `min_radius_factor` — floor for shrinking, as a fraction of the target's
  base radius (default `0.35`), so repeated hits can't shrink it into an
  unhittable point.

Omitting `durability` entirely keeps a target indestructible and
constant-sized (the pre-existing behavior) — this is fully backward
compatible with scenarios that predate the field.

On the destroying hit, the target is immediately hidden and skips ahead to
whatever comes next in its own `sequence` — for a `repeat: true` target with
a single `show` step, that means the very next appearance (often
repositioned, if using `position: { "type": "random", ... }` or a moving
`path`), giving the "shoot it, a fresh one appears" behavior. Two targets
shown simultaneously are always scored and destroyed independently — hitting
one never affects the other's hit count or size. Every fresh appearance of a
target (whether reached by being destroyed early or by simply timing out
normally) resets both `hits_remaining` and its radius to the target's base
values.
