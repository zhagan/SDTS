# Architecture

The system is composed of independent components.

``` text
Scenario
   │
   ▼
 SDTS Core
   ├── Renderer
   ├── Impact Adapter
   ├── Scoring
   └── Recorder
```

Each component communicates using SDTP.

The core owns game state. Renderers display it. Impact adapters observe
reality. Scoring compares the two.
