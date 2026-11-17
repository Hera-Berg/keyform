//! Keyform physics: the pendulum simulation that drives output parameters
//! from input parameters, and physics3.json. No GPU or UI.
//!
//! spec: 70-advanced/physics.md

pub mod edit;
pub mod json3;
pub mod sim;

pub use sim::PhysicsSim;

#[derive(Debug, thiserror::Error)]
pub enum PhysicsError {
    #[error("json: {0}")]
    Json(String),
}

/// Pretty JSON with tab indentation and no trailing newline (reference layout).
pub fn to_json_string(v: &serde_json::Value) -> String {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    if serde::Serialize::serialize(v, &mut ser).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}
