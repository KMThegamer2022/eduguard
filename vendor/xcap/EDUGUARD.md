# EduGuard vendored xcap

This is `xcap` 0.9.8 (MIT/Apache-2.0, see LICENSE) with one change: the
**Linux Wayland/PipeWire capture paths removed**.

Why: xcap's default Linux build links `libwayshot-xcap`, `pipewire`, `zbus`,
`gbm`, `drm` and `wayland-sys` — a large native dependency stack that:

* cannot be cross-compiled without a full Linux sysroot/pkg-config setup, and
* makes the agent binary require libgbm/libpipewire/libwayland at runtime even
  on machines that only run X11.

The rest of xcap's Linux backend is X11/xcb (pure Rust, zero native deps),
which is what EduGuard uses for screenshots and window titles.

Changes vs upstream:

* `src/linux/mod.rs`, `capture.rs`, `impl_video_recorder.rs`,
  `impl_monitor.rs`, `utils.rs`, `src/error.rs`: Wayland/PipeWire/portal code
  and error variants removed; capture is X11-only.
* `src/linux/wayland_capture.rs`, `src/linux/wayland_video_recorder.rs`:
  deleted.
* `Cargo.toml`: removed the `libwayshot-xcap`, `pipewire`, `zbus`, `url`,
  `percent-encoding`, `rand`, `serde` Linux dependencies.

Effect on behaviour: on a Wayland session the agent's screenshot capture
returns an error (logged; the agent keeps running, monitoring keeps working
for heartbeat/inventory). Window titles are X11-only, same as upstream.

To rebase: copy a new xcap release over this directory and re-apply the same
removals, then run `cargo test --workspace` and a Linux build.
