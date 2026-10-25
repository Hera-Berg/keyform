//! Keyform animation playback for runtime files: motions (motion3.json),
//! expressions (exp3.json) and poses (pose3.json). No GPU or UI.
//!
//! spec: 60-animation/motion3-json.md, 70-advanced/viewer.md

pub mod audio;
pub mod expression;
pub mod json;
pub mod motion;
pub mod motionsync;
pub mod player;
pub mod pose;
pub mod tracking;

pub use expression::{ExpBlend, ExpParam, Expression};
pub use motion::{Curve, Motion, Segment, Target};
pub use player::{
    Breath, ExpressionPlayer, EyeBlink, MotionOutputs, MotionPlayer, ParamRange, Params, Rng,
};
pub use pose::{Pose, PoseNode, PoseState};
pub use tracking::{TrackKind, TrackPoint, Tracking};

#[derive(Debug, thiserror::Error)]
pub enum AnimError {
    #[error("json: {0}")]
    Json(String),
    #[error("invalid: {0}")]
    Invalid(String),
}
