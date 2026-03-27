#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    Plain,
    Strong,
    Accent,
    Success,
    Warning,
    Danger,
    Muted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub text: String,
    pub align: Align,
    pub tone: Tone,
}

impl Cell {
    pub fn new(text: impl Into<String>, align: Align, tone: Tone) -> Self {
        Self {
            text: text.into(),
            align,
            tone,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Plain)
    }

    pub fn strong(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Strong)
    }

    pub fn accent(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Accent)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Success)
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Warning)
    }

    pub fn danger(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Danger)
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self::new(text, Align::Left, Tone::Muted)
    }

    pub fn right(text: impl Into<String>, tone: Tone) -> Self {
        Self::new(text, Align::Right, tone)
    }
}

pub fn paint(text: &str, tone: Tone, color: bool) -> String {
    if !color || matches!(tone, Tone::Plain) {
        return text.to_string();
    }

    let code = match tone {
        Tone::Plain => return text.to_string(),
        Tone::Strong => "1",
        Tone::Accent => "36",
        Tone::Success => "32",
        Tone::Warning => "33",
        Tone::Danger => "31",
        Tone::Muted => "2",
    };
    format!("\u{1b}[{code}m{text}\u{1b}[0m")
}

pub fn render_table(headers: &[&str], rows: &[Vec<Cell>], color: bool) -> Vec<String> {
    if headers.is_empty() {
        return Vec::new();
    }

    let mut widths = headers
        .iter()
        .map(|header| display_width(header))
        .collect::<Vec<_>>();

    for row in rows {
        for (index, cell) in row.iter().enumerate().take(widths.len()) {
            widths[index] = widths[index].max(display_width(&cell.text));
        }
    }

    let mut lines = Vec::with_capacity(rows.len() + 4);
    lines.push(render_border('┌', '┬', '┐', &widths));
    lines.push(render_header(headers, &widths, color));
    lines.push(render_border('├', '┼', '┤', &widths));
    for row in rows {
        lines.push(render_row(row, &widths, color));
    }
    lines.push(render_border('└', '┴', '┘', &widths));
    lines
}

fn render_border(left: char, join: char, right: char, widths: &[usize]) -> String {
    let segments = widths
        .iter()
        .map(|width| "─".repeat(width + 2))
        .collect::<Vec<_>>();
    format!("{left}{}{right}", segments.join(&join.to_string()))
}

fn render_header(headers: &[&str], widths: &[usize], color: bool) -> String {
    let cells = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let padded = pad(header, widths[index], Align::Left);
            paint(&padded, Tone::Strong, color)
        })
        .collect::<Vec<_>>();
    format!("│ {} │", cells.join(" │ "))
}

fn render_row(row: &[Cell], widths: &[usize], color: bool) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let cell = row.get(index).cloned().unwrap_or_else(|| Cell::plain(""));
            let padded = pad(&cell.text, *width, cell.align);
            paint(&padded, cell.tone, color)
        })
        .collect::<Vec<_>>();
    format!("│ {} │", cells.join(" │ "))
}

fn pad(value: &str, width: usize, align: Align) -> String {
    let current_width = display_width(value);
    let padding = width.saturating_sub(current_width);
    match align {
        Align::Left => format!("{value}{}", " ".repeat(padding)),
        Align::Right => format!("{}{value}", " ".repeat(padding)),
    }
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
mod tests {
    use super::{Cell, Tone, paint, render_table};

    #[test]
    fn render_table_uses_box_drawing() {
        let table = render_table(
            &["Name", "State"],
            &[vec![Cell::plain("demo"), Cell::success("running")]],
            false,
        );
        assert_eq!(table[0], "┌──────┬─────────┐");
        assert_eq!(table[1], "│ Name │ State   │");
        assert_eq!(table[3], "│ demo │ running │");
        assert_eq!(table[4], "└──────┴─────────┘");
    }

    #[test]
    fn paint_wraps_ansi_sequences_when_enabled() {
        assert_eq!(paint("running", Tone::Success, false), "running");
        assert_eq!(
            paint("running", Tone::Success, true),
            "\u{1b}[32mrunning\u{1b}[0m"
        );
    }
}
