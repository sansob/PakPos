//! The fonts bundled into the app, all under the SIL Open Font License 1.1
//! (licence texts in `assets/fonts/`). egui's default fonts are not used: they
//! include Ubuntu-Light (Ubuntu Font Licence) and Hack (partly Bitstream Vera
//! licence), and every component of a signed release must be under an
//! OSI-approved licence.

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const FONTS: &[(&str, &[u8])] = &[
    ("Inter", include_bytes!("../../../assets/fonts/Inter-Regular.otf")),
    ("JetBrainsMono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf")),
    ("NotoEmoji", include_bytes!("../../../assets/fonts/NotoEmoji-Regular.ttf")),
    ("NotoSansSymbols2", include_bytes!("../../../assets/fonts/NotoSansSymbols2-Regular.ttf")),
    // Only the arrow blocks (⇅ ⟳ …), subset with pyftsubset.
    ("NotoSansMathArrows", include_bytes!("../../../assets/fonts/NotoSansMath-Arrows.ttf")),
];

/// Text font first, then the fallbacks for icons and symbols.
const FALLBACKS: &[&str] = &["NotoEmoji", "NotoSansSymbols2", "NotoSansMathArrows"];

pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    for (name, bytes) in FONTS {
        defs.font_data.insert((*name).to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let family = |first: &[&str]| -> Vec<String> {
        first.iter().chain(FALLBACKS).map(|s| (*s).to_owned()).collect()
    };
    defs.families.insert(FontFamily::Proportional, family(&["Inter"]));
    // Box drawing (─) and other code glyphs Inter lacks come from JetBrains Mono.
    defs.families.insert(FontFamily::Monospace, family(&["JetBrainsMono", "Inter"]));
    if let Some(p) = defs.families.get_mut(&FontFamily::Proportional) {
        p.push("JetBrainsMono".to_owned());
    }
    defs
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_bundled_font_has_its_licence() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        for lic in ["inter", "jetbrainsmono", "notoemoji", "notosanssymbols2", "notosansmath"] {
            let text = std::fs::read_to_string(dir.join(format!("OFL-{lic}.txt"))).unwrap();
            assert!(text.contains("SIL Open Font License"), "{lic}");
        }
    }
}
