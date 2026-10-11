// EduGuard vendored build: Wayland/PipeWire capture paths removed (they pull
// in wayland-sys/gbm/drm/pipewire native dependencies). Linux capture is
// X11/xcb only; on Wayland sessions capture returns an error gracefully.
mod capture;
pub mod utils;
pub mod xorg_capture;
mod xorg_video_recorder;

pub mod impl_monitor;
pub mod impl_video_recorder;
pub mod impl_window;
