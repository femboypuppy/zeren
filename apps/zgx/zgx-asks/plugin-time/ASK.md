# Ask: time spent in each plugin VM

**Priority: low.** A slow plugin badge that updates every frame can't be caught or blamed today.

**Today.** Zeren's SDK can time a node's build only; updates (reactive functions zgx calls later) are invisible to the plugin.

**Ask:** track time in each plugin VM (builds, effects, bound functions) and expose it per contribution in `host.contributions()` (`build_ms`, `update_ms`).

**Seen on:** zgx `b3371f8` (Zeren's pin), macOS. From the plugin chat's notes.
