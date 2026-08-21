use colored::{Color, ColoredString, Colorize};

const ASH: Color = Color::TrueColor { r: 130, g: 130, b: 130 };
const GOLD: Color = Color::TrueColor { r: 181, g: 169, b: 138 };
const SKY: Color = Color::TrueColor { r: 110, g: 160, b: 220 };
const VIOLET: Color = Color::TrueColor { r: 175, g: 145, b: 195 };

pub const AGREES: Style = Style::of(Color::Green);
pub const COMMAND: Style = Style::of(SKY);
pub const COMMENT: Style = Style::of(Color::BrightBlack).italic();
pub const DETAIL: Style = Style::of(Color::BrightBlack);
pub const DIFFERS: Style = Style::of(Color::Red);
pub const FADED: Style = Style::of(ASH);
pub const FLAG: Style = Style::of(Color::Green);
pub const HEADING: Style = Style::plain().bold();
pub const LABEL: Style = Style::of(GOLD).italic();
pub const NUMBER: Style = Style::of(Color::White).bold();
pub const PLAIN: Style = Style::plain();
pub const RECORDED: Style = Style::of(Color::Yellow);
pub const REGION: Style = Style::of(Color::Cyan);
pub const RULE: Style = Style::of(VIOLET);
pub const STRING: Style = Style::of(Color::Green);

// Whether any of this is printed at all is not decided here: `colored` looks at the terminal, at
// NO_COLOR and at CLICOLOR_FORCE, so output to a file or a pipe comes out as plain text.
#[derive(Clone, Copy, Debug, PartialEq)]
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
