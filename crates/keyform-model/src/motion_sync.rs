//! Motion-sync settings of a model (`CMotionSyncSettingSource`).
//! spec: 70-advanced/motion-sync.md §1; layout observed in a reference-made
//! .cmo3 (5.3.04)

/// A Cubism parameter driven by the setting (`SyncCubismParam`).
#[derive(Clone, Debug, PartialEq)]
pub struct SyncCubismParam {
    pub guid: String,
    /// Parameter ID (stored by ID, not by GUID).
    pub parameter_id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub damper: f32,
    pub smooth: i32,
}

/// An audio element (`SyncAudioParam`).
#[derive(Clone, Debug, PartialEq)]
pub struct SyncAudioParam {
    /// The observed files share one null GUID object for all elements.
    pub guid: String,
    pub audio_id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub scale: f32,
    pub enabled: bool,
}

/// The mapping of one audio element (`MappingItem`): its type (`Shape`)
/// and parameter ID → target value, in stored order.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncMapping {
    pub audio_id: String,
    pub kind: String,
    pub targets: Vec<(String, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionSyncSetting {
    pub guid: String,
    pub id: String,
    pub name: String,
    /// `CEMotionSyncVersion`: 0 = 5.0.00 formula, 1 = 5.0.01 and later.
    pub version: u8,
    /// `SyncType` (`CRI`).
    pub sync_type: String,
    /// Post-processing `blendPercentage` as stored (observed 0.5 for the
    /// built-in preset, shown as "Blending Ratio 0.5").
    pub blend: f32,
    pub smoothing: i32,
    pub sample_rate: f32,
    pub cubism: Vec<SyncCubismParam>,
    pub audio: Vec<SyncAudioParam>,
    pub mappings: Vec<SyncMapping>,
}
