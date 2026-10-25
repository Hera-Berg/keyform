//! CJK fallback fonts, loaded from the system at runtime (nothing bundled).
//! spec: 50-ui/theme.md §3 ("CJK fallback fonts required")

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Candidate font files, most preferred first. Plain TrueType files come
/// before variable collections.
fn candidates() -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(target_os = "linux")]
    {
        for lang in ["ja", "zh-cn", "ko"] {
            if let Ok(out) = std::process::Command::new("fc-match")
                .args(["-f", "%{file}", &format!("sans:lang={lang}")])
                .output()
            {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                if !p.is_empty() {
                    v.push(p.into());
                }
            }
        }
        for p in [
            "/usr/share/fonts/google-droid-sans-fonts/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        ] {
            v.push(p.into());
        }
    }
    #[cfg(target_os = "windows")]
    for p in [
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\malgun.ttf",
    ] {
        v.push(p.into());
    }
    #[cfg(target_os = "macos")]
    for p in [
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ] {
        v.push(p.into());
    }
    // Variable fonts last: prefer static ones when both exist.
    v.sort_by_key(|p| p.to_string_lossy().contains("-VF"));
    v.dedup();
    v
}

/// Adds the first readable CJK font as a fallback to both families.
/// Returns its path.
pub fn install(ctx: &egui::Context) -> Option<String> {
    let mut fonts = FontDefinitions::default();
    for path in candidates() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = "keyform-cjk".to_owned();
        fonts
            .font_data
            .insert(name.clone(), FontData::from_owned(bytes).into());
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push(name.clone());
        }
        ctx.set_fonts(fonts);
        return Some(path.display().to_string());
    }
    None
}
