//! System font enumeration for the settings font picker.
//!
//! Scanning the system font directories takes long enough to be visible in a
//! dialog, and the installed set does not change while the app runs, so the
//! result is computed once and cached.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontList {
    /// Families whose faces report fixed advance widths — the ones actually
    /// suitable for a terminal.
    pub monospace: Vec<String>,
    /// Every installed family, for users who want a font the flag missed.
    pub all: Vec<String>,
}

static FONTS: OnceLock<FontList> = OnceLock::new();

/// Whether a face renders every glyph at the same advance width.
///
/// The `monospaced` metadata flag is not trustworthy — macOS ships Monaco with
/// the flag unset — so fall back to measuring. A narrow glyph and a wide one
/// having identical advances is what actually matters for a terminal grid.
fn advances_are_uniform(db: &fontdb::Database, id: fontdb::ID) -> bool {
    db.with_face_data(id, |data, index| {
        let Ok(face) = ttf_parser::Face::parse(data, index) else {
            return false;
        };
        let advance = |c: char| {
            face.glyph_index(c)
                .and_then(|g| face.glyph_hor_advance(g))
                .unwrap_or(0)
        };
        let (i, m) = (advance('i'), advance('M'));
        i != 0 && i == m
    })
    .unwrap_or(false)
}

fn scan() -> FontList {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut monospace = BTreeSet::new();
    let mut all = BTreeSet::new();

    for face in db.faces() {
        // A face lists its family per language; the first entry is the
        // canonical name and the rest are localized aliases we don't want.
        let Some((family, _)) = face.families.first() else {
            continue;
        };
        all.insert(family.clone());
        // Hidden system families (.SF NS Mono and friends) are not selectable.
        if family.starts_with('.') {
            continue;
        }
        if face.monospaced || advances_are_uniform(&db, face.id) {
            monospace.insert(family.clone());
        }
    }

    FontList {
        monospace: monospace.into_iter().collect(),
        all: all.into_iter().collect(),
    }
}

#[tauri::command]
pub fn fonts_list() -> FontList {
    FONTS.get_or_init(scan).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_monospace_families_on_this_machine() {
        let f = scan();
        assert!(!f.all.is_empty(), "no system fonts found at all");
        assert!(!f.monospace.is_empty(), "no monospace fonts detected");
        assert!(f.all.len() >= f.monospace.len());
    }

    /// Monaco ships on macOS with its `monospaced` metadata flag unset, so it
    /// is the regression guard for the advance-measuring fallback.
    #[test]
    #[cfg(target_os = "macos")]
    fn detects_monaco_despite_unset_monospaced_flag() {
        let f = scan();
        assert!(
            f.monospace.iter().any(|n| n == "Monaco"),
            "Monaco missing from monospace list: {:?}",
            f.monospace
        );
    }

    /// Hidden system families are not user-selectable and must stay out.
    #[test]
    fn excludes_dot_prefixed_system_families() {
        let f = scan();
        assert!(!f.monospace.iter().any(|n| n.starts_with('.')));
    }
}
