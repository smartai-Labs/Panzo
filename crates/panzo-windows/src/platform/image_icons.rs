//! Exact SVG paths from docs/prototypes/workbench-v14/app.js.
use super::editor_ui::IconKind;

pub const SELECT: IconKind = IconKind::Prototype(r#"<path d="m5 3 14 10-7 1-3 7z"/>"#);
pub const CROP: IconKind = IconKind::Prototype(r#"<path d="M7 2v15h15M2 7h15v15M7 7h10"/>"#);
pub const PEN: IconKind = IconKind::Prototype(r#"<path d="m4 16 11-11 4 4L8 20H4zM13 7l4 4"/>"#);
pub const RECTANGLE: IconKind = IconKind::Prototype(r#"<path d="M4 5h16v14H4z"/>"#);
pub const ARROW: IconKind = IconKind::Prototype(r#"<path d="M4 19 19 4M7 4h12v12"/>"#);
pub const TEXT: IconKind = IconKind::Prototype(r#"<path d="M4 4h16M12 4v16M8 20h8"/>"#);
pub const BLUR: IconKind =
    IconKind::Prototype(r#"<path d="M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z"/>"#);
pub const OCR: IconKind =
    IconKind::Prototype(r#"<path d="M6 3H3v4m15-4h3v4M3 17v4h3m15-4v4h-3M7 8h10M12 8v9M9 17h6"/>"#);
pub const SPARK: IconKind =
    IconKind::Prototype(r#"<path d="m12 2 2.5 7.5L22 12l-7.5 2.5L12 22l-2.5-7.5L2 12l7.5-2.5z"/>"#);
pub const COLLAGE: IconKind =
    IconKind::Prototype(r#"<path d="M3 3h8v8H3zm10 0h8v8h-8zM3 13h8v8H3zm10 0h8v8h-8z"/>"#);
pub const PIN: IconKind =
    IconKind::Prototype(r#"<path d="m14 3 7 7-4 1-5 5-4-4 5-5zM8 16l-5 5"/>"#);
pub const UNDO: IconKind = IconKind::Prototype(r#"<path d="M8 4 3 9l5 5M3 9h10a6 6 0 0 1 0 12"/>"#);
pub const REDO: IconKind =
    IconKind::Prototype(r#"<path d="m16 4 5 5-5 5m5-5H11a6 6 0 0 0 0 12"/>"#);
pub const REFRESH: IconKind = IconKind::Prototype(r#"<path d="M20 10a8 8 0 1 0 0 5M20 3v7h-7"/>"#);
pub const COPY: IconKind = IconKind::Prototype(r#"<path d="M8 8h12v13H8zM15 8V3H3v13h5"/>"#);
pub const SAVE: IconKind =
    IconKind::Prototype(r#"<path d="M4 3h13l4 4v14H3V3zM7 3v6h9V3M7 21v-8h10v8"/>"#);
pub const MENU: IconKind = IconKind::Prototype(r#"<path d="M4 6h16M4 12h16M4 18h16"/>"#);
pub const CAPTURE: IconKind =
    IconKind::Prototype(r#"<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5M7 7h10v10H7z"/>"#);
pub const FOLDER: IconKind = IconKind::Prototype(r#"<path d="M3 6h7l2 2h9v11H3z"/>"#);
pub const FIT: IconKind =
    IconKind::Prototype(r#"<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>"#);
pub const PANEL: IconKind = IconKind::Prototype(r#"<path d="M4 4h16v16H4zM14 4v16"/>"#);
pub const IMAGE: IconKind =
    IconKind::Prototype(r#"<path d="M4 4h16v16H4zM4 16l5-6 5 6 3-3 3 4M15 8h.01"/>"#);
pub const CLOSE: IconKind = IconKind::Prototype(r#"<path d="m6 6 12 12M18 6 6 18"/>"#);
