//! Keyform's own UI string table. User-visible names that end up in a
//! model (blank-model parts, standard parameter names and descriptions)
//! are localized UI strings; they are looked up here by key in the current
//! UI language. spec: 50-ui/dialogs.md §1.1, psd-import.md §4.1,
//! inspector-fields.md ("localise them through your own string tables")

use std::sync::atomic::{AtomicU8, Ordering};

/// UI languages with a string table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Sets the UI language used by [`text`].
pub fn set_language(l: Language) {
    CURRENT.store(l as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    // English is the only table so far.
    let _ = CURRENT.load(Ordering::Relaxed);
    Language::English
}

/// The string `key` in the current language; the key itself if unknown.
pub fn text(key: &str) -> &str {
    lookup(language(), key).unwrap_or(key)
}

fn lookup(l: Language, key: &str) -> Option<&'static str> {
    let table: &[(&str, &str)] = match l {
        Language::English => EN,
    };
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

const EN: &[(&str, &str)] = &[
    // Blank model parts (dialogs.md §1.1), by part ID.
    ("part.PartRough", "Rough Design"),
    ("part.PartFace", "Face"),
    ("part.PartEye", "Eye"),
    ("part.PartEyeBall", "Eyeball"),
    ("part.PartBrow", "Brows"),
    ("part.PartMouth", "Mouth"),
    ("part.PartNose", "Nose"),
    ("part.PartEar", "Ear"),
    ("part.PartHairFront", "Bangs"),
    ("part.PartHairSide", "Hair Side"),
    ("part.PartHairBack", "Hair Back"),
    ("part.PartNeck", "Neck"),
    ("part.PartBody", "Body"),
    ("part.PartBackground", "Background"),
    ("part.PartSketch", "[ Guide Image]"),
    // Standard parameters (psd-import.md §4.1): name and description.
    ("param.ParamAngleX", "Angle X"),
    (
        "param.ParamAngleX.desc",
        "+ causes it to face right side of screen",
    ),
    ("param.ParamAngleY", "Angle Y"),
    (
        "param.ParamAngleY.desc",
        "+ causes it to face upper side of screen",
    ),
    ("param.ParamAngleZ", "Angle Z"),
    (
        "param.ParamAngleZ.desc",
        "+ causes it to face right side of screen",
    ),
    ("param.ParamEyeLOpen", "EyeL Open"),
    ("param.ParamEyeLOpen.desc", "+ causes eye to open"),
    ("param.ParamEyeLSmile", "EyeL Smile"),
    ("param.ParamEyeLSmile.desc", "+ causes smiley eye"),
    ("param.ParamEyeROpen", "EyeR Open"),
    ("param.ParamEyeROpen.desc", "+ causes eye to open"),
    ("param.ParamEyeRSmile", "EyeR Smile"),
    ("param.ParamEyeRSmile.desc", "+ causes smiley eye"),
    ("param.ParamEyeBallX", "Eyeball X"),
    ("param.ParamEyeBallX.desc", "+ causes it to look right"),
    ("param.ParamEyeBallY", "Eyeball Y"),
    ("param.ParamEyeBallY.desc", "+ causes it to look up"),
    ("param.ParamBrowLY", "BrowL Y"),
    ("param.ParamBrowLY.desc", "+ raises eyebrow"),
    ("param.ParamBrowRY", "BrowR Y"),
    ("param.ParamBrowRY.desc", "+ raises eyebrow"),
    ("param.ParamBrowLX", "BrowL X"),
    ("param.ParamBrowLX.desc", "- moves eyebrow to create frown"),
    ("param.ParamBrowRX", "BrowR X"),
    ("param.ParamBrowRX.desc", "- moves eyebrow to create frown"),
    ("param.ParamBrowLAngle", "BrowL Angle"),
    (
        "param.ParamBrowLAngle.desc",
        "- moves/deforms eyebrow to create scowl",
    ),
    ("param.ParamBrowRAngle", "BrowR Angle"),
    (
        "param.ParamBrowRAngle.desc",
        "- moves/deforms eyebrow to create scowl",
    ),
    ("param.ParamBrowLForm", "BrowL Form"),
    (
        "param.ParamBrowLForm.desc",
        "- moves/deforms eyebrow to create scowl",
    ),
    ("param.ParamBrowRForm", "BrowR Form"),
    (
        "param.ParamBrowRForm.desc",
        "- moves/deforms eyebrow to create scowl",
    ),
    ("param.ParamMouthForm", "Mouth Form"),
    (
        "param.ParamMouthForm.desc",
        "+ causes smiling mouth, - causes scowling mouth",
    ),
    ("param.ParamMouthOpenY", "Mouth Open"),
    ("param.ParamMouthOpenY.desc", "+ causes mouth to open"),
    ("param.ParamCheek", "Cheek"),
    ("param.ParamCheek.desc", "+ causes blush"),
    ("param.ParamBodyAngleX", "Body X"),
    (
        "param.ParamBodyAngleX.desc",
        "+ causes it to face right side of screen",
    ),
    ("param.ParamBodyAngleY", "Body Y"),
    (
        "param.ParamBodyAngleY.desc",
        "+ causes moving it up on the screen",
    ),
    ("param.ParamBodyAngleZ", "Body Z"),
    (
        "param.ParamBodyAngleZ.desc",
        "+ causes tilting it to the right of the screen",
    ),
    ("param.ParamBreath", "Breath"),
    ("param.ParamBreath.desc", "+ causes taking a breath"),
    ("param.ParamHairFront", "Hair Move Front"),
    (
        "param.ParamHairFront.desc",
        "+ causes it to move to the right side of the screen",
    ),
    ("param.ParamHairSide", "Hair Move Side"),
    (
        "param.ParamHairSide.desc",
        "+ causes it to move to the right side of the screen",
    ),
    ("param.ParamHairBack", "Hair Move Back"),
    (
        "param.ParamHairBack.desc",
        "+ causes it to move to the right side of the screen",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_fallback() {
        assert_eq!(text("part.PartFace"), "Face");
        assert_eq!(text("no.such.key"), "no.such.key");
        // No duplicate keys.
        let mut keys: Vec<&str> = EN.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }
}
