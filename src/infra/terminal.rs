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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyValueRow {
    pub key: String,
    pub value: String,
    pub tone: Tone,
}

impl KeyValueRow {
    pub fn new(key: impl Into<String>, value: impl Into<String>, tone: Tone) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            tone,
        }
    }

    pub fn plain(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Plain)
    }

    pub fn accent(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Accent)
    }

    pub fn success(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Success)
    }

    pub fn warning(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Warning)
    }

    pub fn danger(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Danger)
    }

    pub fn muted(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, value, Tone::Muted)
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

pub fn render_key_value_card(title: &str, rows: &[KeyValueRow], color: bool) -> Vec<String> {
    let key_width = rows
        .iter()
        .map(|row| display_width(&row.key))
        .max()
        .unwrap_or(0);
    let value_width = rows
        .iter()
        .map(|row| display_width(&row.value))
        .max()
        .unwrap_or(0);
    let content_width = if rows.is_empty() {
        display_width(title)
    } else if key_width == 0 {
        value_width
    } else {
        key_width + 2 + value_width
    };
    let inner_width = content_width.max(display_width(title));
    let border = "─".repeat(inner_width + 2);

    let mut lines = vec![
        format!("┌{border}┐"),
        format!(
            "│ {} │",
            paint(&pad(title, inner_width, Align::Left), Tone::Strong, color)
        ),
    ];
    if !rows.is_empty() {
        lines.push(format!("├{border}┤"));
        for row in rows {
            lines.push(render_key_value_row(row, key_width, inner_width, color));
        }
    }
    lines.push(format!("└{border}┘"));
    lines
}

pub fn render_tags(tags: &[Cell], color: bool) -> String {
    tags.iter()
        .map(|tag| format!("[{}]", paint(&tag.text, tag.tone, color)))
        .collect::<Vec<_>>()
        .join(" ")
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

fn render_key_value_row(
    row: &KeyValueRow,
    key_width: usize,
    inner_width: usize,
    color: bool,
) -> String {
    let content = if key_width == 0 {
        paint(&pad(&row.value, inner_width, Align::Left), row.tone, color)
    } else {
        let key = paint(&pad(&row.key, key_width, Align::Left), Tone::Muted, color);
        let value_width = inner_width.saturating_sub(key_width + 2);
        let value = paint(&pad(&row.value, value_width, Align::Left), row.tone, color);
        format!("{key}  {value}")
    };
    format!("│ {content} │")
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
    UnicodeWidthStr::width(value)
}

#[cfg(test)]
mod tests {
    use super::{Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table, render_tags};

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

    #[test]
    fn render_key_value_card_uses_box_drawing() {
        let lines = render_key_value_card(
            "Gateway",
            &[
                KeyValueRow::plain("Port", "18789"),
                KeyValueRow::warning("Managed", "loaded"),
            ],
            false,
        );

        assert!(lines[0].starts_with('┌'));
        assert!(lines[1].contains("Gateway"));
        assert!(lines[2].starts_with('├'));
        assert!(lines[3].contains("Port"));
        assert!(lines[3].contains("18789"));
        assert!(lines[4].contains("Managed"));
        assert!(lines[4].contains("loaded"));
        assert!(lines[5].starts_with('└'));
    }

    #[test]
    fn render_tags_formats_painted_badges() {
        let tags = render_tags(
            &[Cell::accent("launcher:dev"), Cell::success("running")],
            false,
        );
        assert_eq!(tags, "[launcher:dev] [running]");
    }
}
use unicode_width::UnicodeWidthStr;
