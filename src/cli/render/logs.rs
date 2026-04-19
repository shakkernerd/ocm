use super::RenderProfile;
use crate::infra::terminal::{KeyValueRow, Tone, paint, render_key_value_card};

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
        "Stream",
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

#[cfg(test)]
mod tests {
    use super::log_header;
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
}
