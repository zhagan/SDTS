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
    "y_mm": 184.0
  }
}
```

For a `result`, `x_mm` and `y_mm` are the impact coordinates, not the target
center. Renderers use these coordinates to draw feedback at the location of
the click. `distance_mm` is the distance between that impact and the target's
authoritative center at scoring time.
