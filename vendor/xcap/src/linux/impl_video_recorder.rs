use std::sync::mpsc::Receiver;

use crate::{XCapResult, video_recorder::Frame};

use super::{impl_monitor::ImplMonitor, xorg_video_recorder::XorgVideoRecorder};

#[derive(Debug, Clone)]
pub enum ImplVideoRecorder {
    Xorg(XorgVideoRecorder),
}

impl ImplVideoRecorder {
    pub fn new(monitor: ImplMonitor) -> XCapResult<(Self, Receiver<Frame>)> {
        let (recorder, receiver) = XorgVideoRecorder::new(monitor)?;
        Ok((ImplVideoRecorder::Xorg(recorder), receiver))
    }

    pub fn start(&self) -> XCapResult<()> {
        match self {
            ImplVideoRecorder::Xorg(recorder) => recorder.start(),
        }
    }

    pub fn stop(&self) -> XCapResult<()> {
        match self {
            ImplVideoRecorder::Xorg(recorder) => recorder.stop(),
        }
    }
}
