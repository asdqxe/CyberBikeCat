# AI assistance and origin

The initial Cargo manifest, terminal motorcycle teaching example and uncompiled
wgpu sketch were supplied by the project owner as Claude-generated material.
The owner keeps the original source separately; this repository is the working copy.

Codex assisted with terminal cleanup, frame pacing, parameter validation, CLI entry
points, the original cat glyphs, the hand-drawn NVIDIA-inspired sign, local event
transport, text controls, tests and documentation. The owner directed the design
and reported local terminal observations. Do not describe this as entirely
human-written code or as a tested integration with Claude Code or Codex.

The historical research mentions campy, buddy-companion and other projects.
Their code and sprite assets were not imported during this implementation.
The renderer does not call a model. Text commands use an explicit local vocabulary;
task events are labelled MANUAL, FAKE, PREVIEW or CHAT. The browser preview runs
a local simulated request; the CHAT label and promise wrapper do not constitute
a real agent adapter. Prompt text stays in the preview page. No real agent adapter
is implemented.

The public validation scope and target-device limitations are documented in
[the README](README.md). Detailed development logs and local run artifacts are
retained separately and are not part of the public source submission. Historical
plans are not evidence of completed features. The wgpu file remains an uncompiled sketch.
