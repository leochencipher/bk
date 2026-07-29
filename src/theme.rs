use crossterm::style::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub fg: Color,
    pub bg: Color,
    pub status_left_fg: Color,
    pub status_left_bg: Color,
    pub status_right_fg: Color,
    pub status_right_bg: Color,
    pub search_highlight: Color,
    pub heading_colors: [Color; 6],
    pub heading_accent_fg: Color,
    pub heading_accent_bg: Color,
    pub bionic_fg: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

pub const THEMES: &[Theme] = &[
    // ── Catppuccin Mocha (dark) ──
    Theme {
        name: "catppuccin-mocha",
        fg: rgb(205, 214, 244),
        bg: rgb(30, 30, 46),
        status_left_fg: rgb(249, 226, 175),
        status_left_bg: rgb(30, 30, 46),
        status_right_fg: rgb(137, 220, 235),
        status_right_bg: rgb(30, 30, 46),
        search_highlight: rgb(250, 179, 135),
        heading_colors: [
            rgb(203, 166, 247), // h1 mauve
            rgb(137, 180, 250), // h2 blue
            rgb(148, 226, 213), // h3 teal
            rgb(166, 227, 161), // h4 green
            rgb(249, 226, 175), // h5 yellow
            rgb(250, 179, 135), // h6 peach
        ],
        heading_accent_fg: rgb(250, 179, 135),
        heading_accent_bg: rgb(49, 50, 68),
        bionic_fg: rgb(250, 179, 135), // peach
    },
    // ── Catppuccin Latte (light) ──
    Theme {
        name: "catppuccin-latte",
        fg: rgb(76, 79, 105),
        bg: rgb(239, 241, 245),
        status_left_fg: rgb(223, 142, 29),
        status_left_bg: rgb(239, 241, 245),
        status_right_fg: rgb(4, 165, 229),
        status_right_bg: rgb(239, 241, 245),
        search_highlight: rgb(254, 100, 11),
        heading_colors: [
            rgb(136, 57, 239),  // h1 mauve
            rgb(30, 102, 245),  // h2 blue
            rgb(23, 146, 153),  // h3 teal
            rgb(64, 160, 43),   // h4 green
            rgb(223, 142, 29),  // h5 yellow
            rgb(254, 100, 11),  // h6 peach
        ],
        heading_accent_fg: rgb(254, 100, 11),
        heading_accent_bg: rgb(204, 208, 218),
        bionic_fg: rgb(254, 100, 11), // peach
    },
    // ── Solarized Dark ──
    Theme {
        name: "solarized-dark",
        fg: rgb(131, 148, 150),
        bg: rgb(0, 43, 54),
        status_left_fg: rgb(181, 137, 0),
        status_left_bg: rgb(0, 43, 54),
        status_right_fg: rgb(42, 161, 152),
        status_right_bg: rgb(0, 43, 54),
        search_highlight: rgb(203, 75, 22),
        heading_colors: [
            rgb(108, 113, 196), // h1 violet
            rgb(38, 139, 210),  // h2 blue
            rgb(42, 161, 152),  // h3 cyan
            rgb(133, 153, 0),   // h4 green
            rgb(181, 137, 0),   // h5 yellow
            rgb(203, 75, 22),   // h6 orange
        ],
        heading_accent_fg: rgb(203, 75, 22),
        heading_accent_bg: rgb(7, 54, 66),
        bionic_fg: rgb(181, 137, 0), // yellow
    },
    // ── Nord ──
    Theme {
        name: "nord",
        fg: rgb(216, 222, 233),
        bg: rgb(46, 52, 64),
        status_left_fg: rgb(235, 203, 139),
        status_left_bg: rgb(46, 52, 64),
        status_right_fg: rgb(136, 192, 208),
        status_right_bg: rgb(46, 52, 64),
        search_highlight: rgb(208, 135, 112),
        heading_colors: [
            rgb(180, 142, 173), // h1 purple
            rgb(94, 129, 172),  // h2 blue
            rgb(143, 188, 187), // h3 teal
            rgb(163, 190, 140), // h4 green
            rgb(235, 203, 139), // h5 yellow
            rgb(208, 135, 112), // h6 orange
        ],
        heading_accent_fg: rgb(208, 135, 112),
        heading_accent_bg: rgb(59, 66, 82),
        bionic_fg: rgb(235, 203, 139), // yellow
    },
    // ── Gruvbox Dark ──
    Theme {
        name: "gruvbox-dark",
        fg: rgb(235, 219, 178),
        bg: rgb(40, 40, 40),
        status_left_fg: rgb(250, 189, 47),
        status_left_bg: rgb(40, 40, 40),
        status_right_fg: rgb(131, 165, 152),
        status_right_bg: rgb(40, 40, 40),
        search_highlight: rgb(254, 128, 25),
        heading_colors: [
            rgb(211, 134, 155), // h1 purple
            rgb(69, 133, 136),  // h2 blue
            rgb(104, 157, 106), // h3 aqua
            rgb(152, 151, 26),  // h4 green
            rgb(250, 189, 47),  // h5 yellow
            rgb(254, 128, 25),  // h6 orange
        ],
        heading_accent_fg: rgb(254, 128, 25),
        heading_accent_bg: rgb(60, 56, 54),
        bionic_fg: rgb(250, 189, 47), // yellow
    },
];

pub fn find_theme(name: &str) -> Option<usize> {
    THEMES.iter().position(|t| t.name == name)
}
