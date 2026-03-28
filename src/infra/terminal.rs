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
        Tone::Strong => "1;38;5;153",
        Tone::Accent => "38;5;81",
        Tone::Success => "38;5;78",
        Tone::Warning => "38;5;221",
        Tone::Danger => "38;5;203",
        Tone::Muted => "38;5;244",
    };
    format!("\u{1b}[{code}m{text}\u{1b}[0m")
}

pub fn render_table(headers: &[&str], rows: &[Vec<Cell>], color: bool) -> Vec<String> {
    render_table_with_limit(headers, rows, color, terminal_width())
}

fn render_table_with_limit(
    headers: &[&str],
    rows: &[Vec<Cell>],
    color: bool,
    max_width: Option<usize>,
) -> Vec<String> {
    if headers.is_empty() {
        return Vec::new();
    }

    let mut widths = headers
        .iter()
        .map(|header| display_width(header))
        .collect::<Vec<_>>();
    let min_widths = widths.clone();

    for row in rows {
        for (index, cell) in row.iter().enumerate().take(widths.len()) {
            widths[index] = widths[index].max(display_width(&cell.text));
        }
    }

    if let Some(max_width) = max_width {
        shrink_widths_to_fit(&mut widths, &min_widths, max_width);
    }

    let mut lines = Vec::with_capacity(rows.len() + 4);
    lines.push(render_border('┌', '┬', '┐', &widths, color));
    lines.push(render_header(headers, &widths, color));
    lines.push(render_border('├', '┼', '┤', &widths, color));
    for row in rows {
        lines.push(render_row(row, &widths, color));
    }
    lines.push(render_border('└', '┴', '┘', &widths, color));
    lines
}

pub fn render_key_value_card(title: &str, rows: &[KeyValueRow], color: bool) -> Vec<String> {
    render_key_value_card_with_limit(title, rows, color, terminal_width())
}

fn render_key_value_card_with_limit(
    title: &str,
    rows: &[KeyValueRow],
    color: bool,
    max_width: Option<usize>,
) -> Vec<String> {
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
    let mut inner_width = content_width.max(display_width(title));
    if let Some(max_width) = max_width {
        inner_width = inner_width.min(max_width.saturating_sub(4));
    }
    let effective_key_width = if rows.is_empty() {
        0
    } else {
        key_width.min(inner_width.saturating_sub(2))
    };
    let border = "─".repeat(inner_width + 2);

    let mut lines = vec![
        border_line('┌', &border, '┐', color),
        format!(
            "{} {} {}",
            border_text("│", color),
            paint(
                &pad_and_truncate(title, inner_width, Align::Left),
                Tone::Strong,
                color
            ),
            border_text("│", color)
        ),
    ];
    if !rows.is_empty() {
        lines.push(border_line('├', &border, '┤', color));
        for row in rows {
            lines.push(render_key_value_row(
                row,
                effective_key_width,
                inner_width,
                color,
            ));
        }
    }
    lines.push(border_line('└', &border, '┘', color));
    lines
}

pub fn render_tags(tags: &[Cell], color: bool) -> String {
    tags.iter()
        .map(|tag| format!("[{}]", paint(&tag.text, tag.tone, color)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_border(left: char, join: char, right: char, widths: &[usize], color: bool) -> String {
    let segments = widths
        .iter()
        .map(|width| "─".repeat(width + 2))
        .collect::<Vec<_>>();
    border_text(
        &format!("{left}{}{right}", segments.join(&join.to_string())),
        color,
    )
}

fn render_header(headers: &[&str], widths: &[usize], color: bool) -> String {
    let cells = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let padded = pad_and_truncate(header, widths[index], Align::Left);
            paint(&padded, Tone::Strong, color)
        })
        .collect::<Vec<_>>();
    format!(
        "{} {} {}",
        border_text("│", color),
        cells.join(&border_text(" │ ", color)),
        border_text("│", color)
    )
}

fn render_row(row: &[Cell], widths: &[usize], color: bool) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let cell = row.get(index).cloned().unwrap_or_else(|| Cell::plain(""));
            let padded = pad_and_truncate(&cell.text, *width, cell.align);
            paint(&padded, cell.tone, color)
        })
        .collect::<Vec<_>>();
    format!(
        "{} {} {}",
        border_text("│", color),
        cells.join(&border_text(" │ ", color)),
        border_text("│", color)
    )
}

fn render_key_value_row(
    row: &KeyValueRow,
    key_width: usize,
    inner_width: usize,
    color: bool,
) -> String {
    let content = if key_width == 0 {
        paint(
            &pad_and_truncate(&row.value, inner_width, Align::Left),
            row.tone,
            color,
        )
    } else {
        let key = paint(
            &pad_and_truncate(&row.key, key_width, Align::Left),
            Tone::Muted,
            color,
        );
        let value_width = inner_width.saturating_sub(key_width + 2);
        let value = paint(
            &pad_and_truncate(&row.value, value_width, Align::Left),
            row.tone,
            color,
        );
        format!("{key}  {value}")
    };
    format!(
        "{} {} {}",
        border_text("│", color),
        content,
        border_text("│", color)
    )
}

fn border_text(text: &str, color: bool) -> String {
    paint(text, Tone::Muted, color)
}

fn border_line(left: char, center: &str, right: char, color: bool) -> String {
    border_text(&format!("{left}{center}{right}"), color)
}

fn pad(value: &str, width: usize, align: Align) -> String {
    let current_width = display_width(value);
    let padding = width.saturating_sub(current_width);
    match align {
        Align::Left => format!("{value}{}", " ".repeat(padding)),
        Align::Right => format!("{}{value}", " ".repeat(padding)),
    }
}

fn pad_and_truncate(value: &str, width: usize, align: Align) -> String {
    pad(&truncate_for_width(value, width, align), width, align)
}

fn truncate_for_width(value: &str, width: usize, align: Align) -> String {
    if display_width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }

    match align {
        Align::Left => truncate_middle(value, width),
        Align::Right => truncate_from_left(value, width),
    }
}

fn truncate_middle(value: &str, width: usize) -> String {
    let head_width = (width - 1) / 2;
    let tail_width = width - 1 - head_width;
    format!(
        "{}…{}",
        take_prefix_width(value, head_width),
        take_suffix_width(value, tail_width)
    )
}

fn truncate_from_left(value: &str, width: usize) -> String {
    format!("…{}", take_suffix_width(value, width.saturating_sub(1)))
}

fn take_prefix_width(value: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        out.push(ch);
    }
    out
}

fn take_suffix_width(value: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = Vec::new();
    for ch in value.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        out.push(ch);
    }
    out.into_iter().rev().collect()
}

pub fn terminal_width() -> Option<usize> {
    if let Some(width) = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
    {
        return width.checked_sub(1).or(Some(width));
    }

    terminal_size::terminal_size().and_then(|(terminal_size::Width(width), _)| {
        let width = usize::from(width);
        width.checked_sub(1).or(Some(width))
    })
}

fn shrink_widths_to_fit(widths: &mut [usize], min_widths: &[usize], max_width: usize) {
    while table_width(widths) > max_width {
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(index, width)| **width > min_widths[*index])
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        widths[index] -= 1;
    }
}

fn table_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + (widths.len() * 3) + 1
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

#[cfg(test)]
mod tests {
    use super::{
        Cell, KeyValueRow, Tone, display_width, paint, render_key_value_card,
        render_key_value_card_with_limit, render_table, render_table_with_limit, render_tags,
    };

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
            "\u{1b}[38;5;78mrunning\u{1b}[0m"
        );
    }

    #[test]
    fn render_table_colors_borders_and_headers_when_enabled() {
        let table = render_table(
            &["Name", "State"],
            &[vec![Cell::plain("demo"), Cell::success("running")]],
            true,
        );

        assert!(table[0].contains("\u{1b}[38;5;244m"));
        assert!(table[1].contains("\u{1b}[1;38;5;153mName"));
        assert!(table[3].contains("\u{1b}[38;5;78mrunning"));
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
    fn render_key_value_card_truncates_wide_values_to_fit_the_available_width() {
        let lines = render_key_value_card_with_limit(
            "Managed service",
            &[KeyValueRow::plain(
                "Plist",
                "/Users/shakker/Library/LaunchAgents/ai.openclaw.gateway.ocm.hacking.plist",
            )],
            false,
            Some(60),
        );

        assert!(lines.iter().all(|line| display_width(line) <= 60));
        assert!(lines[3].contains('…'));
    }

    #[test]
    fn render_tags_formats_painted_badges() {
        let tags = render_tags(
            &[Cell::accent("launcher:dev"), Cell::success("running")],
            false,
        );
        assert_eq!(tags, "[launcher:dev] [running]");
    }

    #[test]
    fn render_table_truncates_wide_cells_to_fit_the_available_width() {
        let table = render_table_with_limit(
            &["Name", "Command", "Cwd"],
            &[vec![
                Cell::plain("dev"),
                Cell::plain("pnpm openclaw gateway run --port 18791"),
                Cell::muted("/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/openclaw"),
            ]],
            false,
            Some(60),
        );

        assert!(table.iter().all(|line| display_width(line) <= 60));
        assert!(table[3].contains('…'));
    }
}
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
