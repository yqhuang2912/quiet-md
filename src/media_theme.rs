//! Optional reading styles for images and code, independent of the document
//! theme. Values come from manbianliang-notes/dist/notes/styles.css (not its
//! block editor). Tables, body text and the application chrome keep their theme.

use crate::themes::Style;
use gpui::Hsla;
use std::sync::LazyLock;

const CSS: &str = include_str!("../assets/content-styles/manbianliang.css");
static MANBIANLIANG: LazyLock<Style> = LazyLock::new(|| {
    let fonts = vec![if cfg!(target_os = "windows") { "Consolas" } else { "Monaco" }.into()];
    crate::themes::parse(CSS, &fonts).expect("bundled media style").0
});

pub fn active() -> Option<&'static Style> {
    if crate::settings::manbianliang() { Some(&MANBIANLIANG) } else { None }
}

pub fn code_style() -> &'static Style { active().unwrap_or_else(|| crate::editor_theme::style()) }
pub fn mono_font() -> &'static str { code_style().font("pre.font") }

// Compiled reading page: square figure, 1px neutral rim, 3px inset; caption
// separated by a 1px rule, 3px gap/inset, 12px / 1.6, normal weight.
pub const FIGURE_INSET: f32 = 4.;
pub const CAPTION_GAP: f32 = 3.;
pub const CAPTION_SIZE: f32 = 12.;
pub const CAPTION_LINE: f32 = 19.2;
pub fn figure_border() -> Hsla { crate::theme::c(0xe5e5e5) }
pub fn caption_border() -> Hsla { crate::theme::c(0xe8e8e8) }
pub fn caption_color() -> Hsla { crate::theme::c(0x666666) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_preset_is_scoped_to_code() {
        let (st, _, warnings) = crate::themes::parse(CSS, &[]).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((st.num("pre.size"), st.num("pre.line"), st.num("pre.radius")), (14., 21., 0.));
        // Every declaration is restricted to code selectors. In particular,
        // applying this preset cannot introduce a table or page colour.
        for rule in CSS.split('}').filter_map(|r| r.rsplit_once('{')) {
            let selector = rule.0.rsplit("*/").next().unwrap().trim();
            assert!(selector == ":root" || selector == "code" || selector == "pre" || selector == ".line-numbers" || selector.starts_with(".token."), "{selector}");
        }
        assert_eq!(st.num("table.size"), Style::plain(false).num("table.size"));
    }
}
