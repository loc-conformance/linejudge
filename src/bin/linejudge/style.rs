use colored::{Color, ColoredString, Colorize};

// The gold of mezura's own labels, since the two tools are read in the same terminal minutes apart.
const GOLD: Color = Color::TrueColor { r: 181, g: 169, b: 138 };
const VIOLET: Color = Color::TrueColor { r: 175, g: 145, b: 195 };

pub const AGREES: Style = Style::of(Color::Green);
pub const COMMENT: Style = Style::of(Color::BrightBlack).italic();
pub const DETAIL: Style = Style::of(Color::BrightBlack);
pub const DIFFERS: Style = Style::of(Color::Red);
pub const HEADING: Style = Style::plain().bold();
pub const LABEL: Style = Style::of(GOLD).italic();
pub const NUMBER: Style = Style::of(Color::White).bold();
pub const RECORDED: Style = Style::of(Color::Yellow);
pub const REGION: Style = Style::of(Color::Cyan);
pub const RULE: Style = Style::of(VIOLET);
pub const STRING: Style = Style::of(Color::Green);

/// One colour and whatever else a terminal cell can carry. Whether any of it is printed at all is
/// not decided here: `colored` looks at the terminal, at NO_COLOR and at CLICOLOR_FORCE once, and
/// a run whose output is a file or a pipe comes out as plain text.
#[derive(Clone, Copy)]
pub struct Style {
    color: Option<Color>,
    bold: bool,
    italic: bool,
}

impl Style {
    pub fn paint(&self, text: &str) -> ColoredString {
        let mut painted = ColoredString::from(text);
        if let Some(color) = self.color {
            painted = painted.color(color);
        }
        if self.bold {
            painted = painted.bold();
        }
        if self.italic {
            painted = painted.italic();
        }
        painted
    }

    const fn of(color: Color) -> Style {
        Style { color: Some(color), bold: false, italic: false }
    }

    const fn plain() -> Style {
        Style { color: None, bold: false, italic: false }
    }

    const fn bold(self) -> Style {
        Style { bold: true, ..self }
    }

    const fn italic(self) -> Style {
        Style { italic: true, ..self }
    }
}
