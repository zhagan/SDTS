# SDTP 0.1 --- Software Defined Target Protocol (Draft)

> **Status:** Early design draft\
> **Philosophy:** Discover the protocol through experimentation, not
> speculation.

## Why another protocol?

SDTP is the communication protocol used by the **Software Defined Target
System (SDTS)**.

The goal is not to define every possible message up front. Instead, the
protocol should remain intentionally small and evolve as new hardware,
renderers, and impact detectors are added.

The protocol exists so that components can communicate without caring
whether they are written in Rust, C++, Python, JavaScript, or another
language.

------------------------------------------------------------------------

# Design Principles

-   Keep the protocol small.
-   Make experimentation easy.
-   Be language independent.
-   Prefer convention over specification.
-   Add structure only when multiple components truly need it.
-   Keep hardware-specific details inside adapters.

------------------------------------------------------------------------

# Transport

Initial implementation:

-   WebSocket
-   JSON
-   One JSON object per message

This keeps debugging simple and allows browser tools to participate
immediately.

------------------------------------------------------------------------

# Message Envelope

Every message shares the same minimal structure.

``` json
{
  "type": "impact",
  "time": 12.482,
  "source": "mouse",
  "data": {
    "x": 542.1,
    "y": 608.2
  }
}
```

Required fields:

  Field      Description
  ---------- ---------------------------------
  `type`     Message type
  `time`     Seconds since session start
  `source`   Component producing the message
  `data`     Flexible payload

Optional fields:

  Field        Description
  ------------ ---------------------------
  `id`         Unique message identifier
  `reply_to`   Response correlation

------------------------------------------------------------------------

# Units

The protocol defines a few universal conventions.

## Time

-   Seconds from session start

## Coordinates

-   Millimeters

Example:

``` json
{
  "position": [610.0, 305.0]
}
```

------------------------------------------------------------------------

# Initial Message Types

The first implementation only needs a handful of messages.

## hello

Component startup.

## target

Spawn or modify targets.

## impact

An impact detector reports a measured impact.

## score

Result of evaluating an impact.

## render

Geometry to display.

## control

Operator actions such as:

-   start
-   stop
-   pause
-   reset
-   blank

------------------------------------------------------------------------

# Examples

## Target

``` json
{
  "type": "target",
  "time": 2.0,
  "source": "scenario",
  "data": {
    "id": "t1",
    "shape": "circle",
    "position": [100, 610],
    "radius": 75
  }
}
```

## Impact

``` json
{
  "type": "impact",
  "time": 4.81,
  "source": "mouse",
  "data": {
    "position": [526, 608],
    "confidence": 1.0
  }
}
```

## Score

``` json
{
  "type": "score",
  "time": 4.81,
  "source": "core",
  "data": {
    "target": "t1",
    "result": "hit",
    "distance": 21.4,
    "points": 1
  }
}
```

------------------------------------------------------------------------

# Protocol Evolution

The protocol should evolve only when experience demonstrates a real
need.

Examples of future additions might include:

-   Calibration
-   Safety
-   Replay
-   Clock synchronization
-   Capability negotiation
-   Advanced target descriptions

These should **not** be added until they are justified by working
software.

------------------------------------------------------------------------

# Guiding Rule

> Add protocol structure only after two independent components genuinely
> need to agree on it.

This keeps SDTP lightweight while allowing it to grow naturally.

The protocol should never become more complicated than the system it
supports.
