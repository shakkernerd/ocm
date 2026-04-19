use super::RenderProfile;
use crate::infra::terminal::{KeyValueRow, Tone, paint, render_key_value_card};
use time::OffsetDateTime;

pub fn log_header(
    env_name: &str,
    stream: &str,
    source_kind: &str,
    path: &str,
    tail_lines: Option<usize>,
    follow: bool,
    profile: RenderProfile,
) -> Vec<String> {
    if !profile.pretty {
        return Vec::new();
    }

    let mut lines = vec![paint(
        &format!("Logs {}", env_name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Active log",
        &[
            KeyValueRow::plain("Stream", stream.to_string()),
            KeyValueRow::plain("Source", source_kind.to_string()),
            KeyValueRow::plain("Path", path.to_string()),
            KeyValueRow::plain(
                "Mode",
                if follow {
                    "follow".to_string()
                } else {
                    "snapshot".to_string()
                },
            ),
            KeyValueRow::plain(
                "Tail",
                tail_lines
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "all".to_string()),
            ),
        ],
        profile.color,
    ));
    lines.push(String::new());
    lines
}

pub fn render_log_text(text: &str, profile: RenderProfile) -> String {
    if !profile.pretty {
        return text.to_string();
    }

    text.split_inclusive('\n')
        .map(|line| render_log_line(line, profile))
        .collect::<String>()
}

fn render_log_line(line: &str, profile: RenderProfile) -> String {
    let has_newline = line.ends_with('\n');
    let body = line.trim_end_matches('\n');
    if body.is_empty() {
        return line.to_string();
    }

    let rendered = parse_bracketed_line(body)
        .or_else(|| parse_leveled_line(body))
        .map(|parsed| format_parsed_line(parsed, profile))
        .unwrap_or_else(|| body.to_string());

    if has_newline {
        format!("{rendered}\n")
    } else {
        rendered
    }
}

#[derive(Clone, Copy)]
struct ParsedLogLine<'a> {
    timestamp: Option<&'a str>,
    level: Option<&'a str>,
    source: Option<&'a str>,
    message: &'a str,
    bracket_source: bool,
}

fn parse_bracketed_line(line: &str) -> Option<ParsedLogLine<'_>> {
    let (timestamp, rest) = line.split_once(' ')?;
    if !looks_like_timestamp(timestamp) {
        return None;
    }
    let rest = rest.strip_prefix('[')?;
    let end = rest.find(']')?;
    let source = &rest[..end];
    let message = rest[end + 1..].trim_start();
    Some(ParsedLogLine {
        timestamp: Some(timestamp),
        level: None,
        source: Some(source),
        message,
        bracket_source: true,
    })
}

fn parse_leveled_line(line: &str) -> Option<ParsedLogLine<'_>> {
    let (timestamp, rest) = line.split_once(' ')?;
    if !looks_like_timestamp(timestamp) {
        return None;
    }

    let mut parts = rest.splitn(3, ' ');
    let level = parts.next()?;
    if !is_log_level(level) {
        return None;
    }
    let source = parts.next();
    let message = parts.next().unwrap_or("").trim_start();
    Some(ParsedLogLine {
        timestamp: Some(timestamp),
        level: Some(level),
        source,
        message,
        bracket_source: false,
    })
}

fn format_parsed_line(parsed: ParsedLogLine<'_>, profile: RenderProfile) -> String {
    let mut parts = Vec::new();
    if let Some(timestamp) = parsed.timestamp {
        parts.push(paint(
            &format_log_timestamp(timestamp),
            Tone::Muted,
            profile.color,
        ));
    }
    if let Some(level) = parsed.level {
        parts.push(paint(level, level_tone(level), profile.color));
    }
    if let Some(source) = parsed.source {
        let source_label = if parsed.bracket_source {
            format!("[{source}]")
        } else {
            source.to_string()
        };
        parts.push(paint(&source_label, Tone::Accent, profile.color));
    }
    if !parsed.message.is_empty() {
        parts.push(paint(
            parsed.message,
            parsed.level.map(level_tone).unwrap_or(Tone::Plain),
            profile.color,
        ));
    }
    parts.join(" ")
}

fn format_log_timestamp(raw: &str) -> String {
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
        .map(|value| {
            value
                .format(
                    &time::format_description::parse(
                        "[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]",
                    )
                    .unwrap(),
                )
                .unwrap_or_else(|_| raw.to_string())
        })
        .unwrap_or_else(|_| raw.to_string())
}

fn looks_like_timestamp(value: &str) -> bool {
    value
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
        && value.contains(':')
}

fn is_log_level(value: &str) -> bool {
    matches!(
        value,
        "trace" | "debug" | "info" | "warn" | "error" | "fatal"
    )
}

fn level_tone(level: &str) -> Tone {
    match level {
        "error" | "fatal" => Tone::Danger,
        "warn" => Tone::Warning,
        "debug" | "trace" => Tone::Muted,
        "info" => Tone::Accent,
        _ => Tone::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::{log_header, render_log_text};
    use crate::cli::render::RenderProfile;

    #[test]
    fn log_header_pretty_uses_cards() {
        let lines = log_header(
            "demo",
            "stdout",
            "gateway",
            "/tmp/demo/.openclaw/logs/gateway.log",
            Some(50),
            false,
            RenderProfile::pretty(false),
        );
        assert!(lines.iter().any(|line| line.contains("Logs demo")));
        assert!(lines.iter().any(|line| line.contains("gateway.log")));
        assert!(lines.iter().any(|line| line.contains("snapshot")));
    }

    #[test]
    fn render_log_text_pretty_formats_common_openclaw_log_shapes() {
        let rendered = render_log_text(
            concat!(
                "2026-04-20T00:13:45.497+01:00 [agents/tool-images] Image resized\n",
                "04:42:38+00:00 error gateway connect failed\n"
            ),
            RenderProfile::pretty(false),
        );
        assert!(rendered.contains("00:13:45+01:00 [agents/tool-images] Image resized"));
        assert!(rendered.contains("04:42:38+00:00 error gateway connect failed"));
    }
}
