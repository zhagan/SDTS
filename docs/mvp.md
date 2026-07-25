# MVP

## Goal

Prove the architecture before buying additional hardware.

## Components

-   Rust SDTS Core
-   Browser renderer
-   Mouse impact adapter

## Flow

1.  Spawn a moving circle.
2.  Render in browser.
3.  Click to simulate impacts.
4.  Compute hit/miss.
5.  Record every event.

## Success Criteria

-   Circle moves deterministically.
-   Replay reproduces identical motion.
-   Hits are scored consistently.
