//! Pinned Lucide path data used by Zode's semantic icon registry.
//!
//! Source: Lucide 1.24.0 (`b5b5d95933790a311aa6b7ed232fc8469934acdf`).
//! <https://github.com/lucide-icons/lucide/tree/1.24.0/icons>
//!
//! The upstream SVG children are normalized into one 24×24 `d` string per
//! icon because Jian's renderer-facing `Painter` accepts path data rather than
//! complete SVG documents. Lines are expressed as paths, while circles and
//! rounded rectangles are expressed with equivalent arcs. See the repository
//! `THIRD_PARTY_NOTICES.md` for the Lucide and Feather license notices.

pub(super) const SQUARE_PEN: &str = concat!(
    "M12 3H5A2 2 0 0 0 3 5V19A2 2 0 0 0 5 21H19A2 2 0 0 0 21 19V12 ",
    "M18.375 2.625A1 1 0 0 1 21.375 5.625L12.362 14.639A2 2 0 0 1 11.509 15.144L8.636 15.984A.5.5 0 0 1 8.016 15.364L8.856 12.491A2 2 0 0 1 9.362 11.639Z"
);

pub(super) const CLOCK: &str = "M22 12A10 10 0 1 1 2 12A10 10 0 1 1 22 12Z M12 6V12L16 14";

pub(super) const SEARCH: &str = "M21 21L16.66 16.66 M19 11A8 8 0 1 1 3 11A8 8 0 1 1 19 11Z";

pub(super) const AT_SIGN: &str = concat!(
    "M16 12A4 4 0 1 1 8 12A4 4 0 1 1 16 12Z ",
    "M16 8V13A3 3 0 0 0 22 13V12A10 10 0 1 0 18 20"
);

pub(super) const BLOCKS: &str = concat!(
    "M10 22V7A1 1 0 0 0 9 6H4A2 2 0 0 0 2 8V20A2 2 0 0 0 4 22H16A2 2 0 0 0 18 20V15A1 1 0 0 0 17 14H2 ",
    "M15 2H21A1 1 0 0 1 22 3V9A1 1 0 0 1 21 10H15A1 1 0 0 1 14 9V3A1 1 0 0 1 15 2Z"
);

pub(super) const GIT_PULL_REQUEST: &str = concat!(
    "M21 18A3 3 0 1 1 15 18A3 3 0 1 1 21 18Z ",
    "M9 6A3 3 0 1 1 3 6A3 3 0 1 1 9 6Z ",
    "M13 6H16A2 2 0 0 1 18 8V15 M6 9V21"
);

pub(super) const MESSAGE_CIRCLE_PLUS: &str = concat!(
    "M2.992 16.342A2 2 0 0 1 3.086 17.509L2.021 20.799A1 1 0 0 0 3.257 21.967L6.67 20.969A2 2 0 0 1 7.769 21.061A10 10 0 1 0 2.992 16.342 ",
    "M8 12H16 M12 8V16"
);

pub(super) const FOLDER: &str =
    "M20 20A2 2 0 0 0 22 18V8A2 2 0 0 0 20 6H12.1A2 2 0 0 1 10.41 5.1L9.6 3.9A2 2 0 0 0 7.93 3H4A2 2 0 0 0 2 5V18A2 2 0 0 0 4 20Z";

pub(super) const PIN: &str = concat!(
    "M12 17V22 ",
    "M9 10.76A2 2 0 0 1 7.89 12.55L6.11 13.45A2 2 0 0 0 5 15.24V16A1 1 0 0 0 6 17H18A1 1 0 0 0 19 16V15.24A2 2 0 0 0 17.89 13.45L16.11 12.55A2 2 0 0 1 15 10.76V7A1 1 0 0 1 16 6A2 2 0 0 0 16 2H8A2 2 0 0 0 8 6A1 1 0 0 1 9 7Z"
);

pub(super) const ARCHIVE: &str = concat!(
    "M3 3H21A1 1 0 0 1 22 4V7A1 1 0 0 1 21 8H3A1 1 0 0 1 2 7V4A1 1 0 0 1 3 3Z ",
    "M4 8V19A2 2 0 0 0 6 21H18A2 2 0 0 0 20 19V8 M10 12H14"
);

pub(super) const ELLIPSIS: &str = concat!(
    "M13 12A1 1 0 1 1 11 12A1 1 0 1 1 13 12Z ",
    "M20 12A1 1 0 1 1 18 12A1 1 0 1 1 20 12Z ",
    "M6 12A1 1 0 1 1 4 12A1 1 0 1 1 6 12Z"
);

pub(super) const PENCIL: &str = concat!(
    "M21.174 6.812A1 1 0 0 0 17.188 2.825L3.842 16.174A2 2 0 0 0 3.342 17.004L2.021 21.356A.5.5 0 0 0 2.644 21.978L6.997 20.658A2 2 0 0 0 7.827 20.161Z ",
    "M15 5L19 9"
);

pub(super) const CHEVRON_RIGHT: &str = "M9 18L15 12L9 6";
pub(super) const CHEVRON_DOWN: &str = "M6 9L12 15L18 9";
pub(super) const X: &str = "M18 6L6 18 M6 6L18 18";
pub(super) const CHECK: &str = "M20 6L9 17L4 12";

pub(super) const EXTERNAL_LINK: &str =
    "M15 3H21V9 M10 14L21 3 M18 13V19A2 2 0 0 1 16 21H5A2 2 0 0 1 3 19V8A2 2 0 0 1 5 6H11";

pub(super) const REFRESH_CW: &str = concat!(
    "M3 12A9 9 0 0 1 12 3A9.75 9.75 0 0 1 18.74 5.74L21 8 ",
    "M21 3V8H16 ",
    "M21 12A9 9 0 0 1 12 21A9.75 9.75 0 0 1 5.26 18.26L3 16 ",
    "M8 16H3V21"
);

pub(super) const SETTINGS: &str = concat!(
    "M9.671 4.136A2.34 2.34 0 0 1 14.33 4.136A2.34 2.34 0 0 0 17.649 6.051A2.34 2.34 0 0 1 19.979 10.084A2.34 2.34 0 0 0 19.979 13.915A2.34 2.34 0 0 1 17.649 17.948A2.34 2.34 0 0 0 14.33 19.863A2.34 2.34 0 0 1 9.671 19.863A2.34 2.34 0 0 0 6.351 17.948A2.34 2.34 0 0 1 4.021 13.915A2.34 2.34 0 0 0 4.021 10.084A2.34 2.34 0 0 1 6.35 6.051A2.34 2.34 0 0 0 9.669 4.136 ",
    "M15 12A3 3 0 1 1 9 12A3 3 0 1 1 15 12Z"
);

pub(super) const CIRCLE_QUESTION_MARK: &str = concat!(
    "M22 12A10 10 0 1 1 2 12A10 10 0 1 1 22 12Z ",
    "M9.09 9A3 3 0 0 1 14.92 10C14.92 12 11.92 13 11.92 13 M12 17H12.01"
);

pub(super) const PLUS: &str = "M5 12H19 M12 5V19";
