fn format_entries(entries: &[(&str, &str)]) -> Vec<String> {
    let width = entries
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    entries
        .iter()
        .map(|(name, description)| format!("  {:width$}  {}", name, description, width = width))
        .collect()
}

fn format_usage(entries: &[String]) -> Vec<String> {
    entries.iter().map(|entry| format!("  {entry}")).collect()
}

fn format_examples(entries: &[String]) -> Vec<String> {
    entries.iter().map(|entry| format!("  {entry}")).collect()
}

fn format_notes(entries: &[&str]) -> Vec<String> {
    entries.iter().map(|entry| format!("  {entry}")).collect()
}

fn push_section(lines: &mut Vec<String>, title: &str, body: Vec<String>) {
    if body.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("{title}:"));
    lines.extend(body);
}

fn finish(lines: Vec<String>) -> String {
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn render_leaf(
    title: &str,
    summary: &str,
    usage: Vec<String>,
    options: &[(&str, &str)],
    examples: Vec<String>,
    notes: &[&str],
) -> String {
    let mut lines = vec![title.to_string(), String::new(), summary.to_string()];
    push_section(&mut lines, "Usage", format_usage(&usage));
    push_section(&mut lines, "Options", format_entries(options));
    push_section(&mut lines, "Examples", format_examples(&examples));
    push_section(&mut lines, "Notes", format_notes(notes));
    finish(lines)
}

fn render_group(
    title: &str,
    summary: &str,
    usage: Vec<String>,
    sections: &[(&str, &[(&str, &str)])],
    examples: Vec<String>,
    more: Vec<String>,
) -> String {
    let mut lines = vec![title.to_string(), String::new(), summary.to_string()];
    push_section(&mut lines, "Usage", format_usage(&usage));
    for (title, entries) in sections {
        push_section(&mut lines, title, format_entries(entries));
    }
    push_section(&mut lines, "Examples", format_examples(&examples));
    push_section(&mut lines, "More", format_examples(&more));
    finish(lines)
}

pub fn root_help(cmd: &str) -> String {
    let lines = vec![
        "OpenClaw Manager".to_string(),
        String::new(),
        "Manage isolated OpenClaw environments, runtimes, launchers, and services.".to_string(),
    ];
    let mut lines = lines;
    push_section(
        &mut lines,
        "Usage",
        format_usage(&[
            format!("{cmd} <command> [args]"),
            format!("{cmd} help <command>"),
        ]),
    );
    push_section(
        &mut lines,
        "Commands",
        format_entries(&[
            (
                "env",
                "Environment lifecycle, binding, execution, snapshots, and repair",
            ),
            ("launcher", "Named command recipes for running OpenClaw"),
            (
                "runtime",
                "Registered and installer-managed OpenClaw runtimes",
            ),
            ("service", "Persistent OpenClaw services for environments"),
            ("init", "Shell setup snippets for using ocm"),
            ("help", "Show help for a command or command group"),
            ("--version", "Show the installed ocm version"),
        ]),
    );
    push_section(
        &mut lines,
        "Get started",
        format_examples(&[
            format!("{cmd} launcher add stable --command openclaw"),
            format!("{cmd} env create demo --launcher stable"),
            format!("eval \"$({cmd} env use demo)\""),
            format!("{cmd} -- status"),
            format!("{cmd} @demo -- status"),
            format!("{cmd} env run demo -- onboard"),
        ]),
    );
    push_section(
        &mut lines,
        "More",
        format_examples(&[
            format!("{cmd} help env"),
            format!("{cmd} help service"),
            format!("{cmd} help runtime install"),
        ]),
    );
    finish(lines)
}

pub fn init_help(cmd: &str) -> String {
    render_leaf(
        "Shell init snippets",
        "Print shell integration for making ocm activation easier to use.",
        vec![
            format!("{cmd} init [zsh|bash|sh|fish]"),
            format!("{cmd} help init"),
        ],
        &[(
            "[zsh|bash|sh|fish]",
            "Optional shell override. Bare `init` auto-detects the current shell.",
        )],
        vec![
            format!("{cmd} init"),
            format!("{cmd} init zsh"),
            format!("{cmd} init fish"),
        ],
        &[
            "This command prints shell code to stdout.",
            "Use it from your shell rc file or evaluate it explicitly.",
        ],
    )
}

pub fn env_help(cmd: &str) -> String {
    render_group(
        "Environment commands",
        "Create, inspect, bind, run, snapshot, and repair isolated OpenClaw environments.",
        vec![
            format!("{cmd} env <command> [args]"),
            format!("{cmd} help env <command>"),
        ],
        &[
            (
                "Lifecycle",
                &[
                    ("create", "Create an environment"),
                    ("clone", "Clone an environment"),
                    ("list", "List environments"),
                    ("show", "Show environment metadata"),
                    ("remove", "Remove an environment"),
                    ("prune", "Preview or remove old environments"),
                ],
            ),
            (
                "Binding",
                &[
                    ("set-launcher", "Bind or clear a launcher"),
                    ("set-runtime", "Bind or clear a runtime"),
                    ("resolve", "Show what the environment would run"),
                ],
            ),
            (
                "Execution",
                &[
                    ("use", "Emit shell activation for an environment"),
                    ("exec", "Run any command inside an environment"),
                    ("run", "Run OpenClaw inside an environment"),
                    ("status", "Show environment runtime and service status"),
                ],
            ),
            (
                "Health",
                &[
                    ("doctor", "Inspect environment problems"),
                    ("cleanup", "Preview or apply safe repairs"),
                    ("repair-marker", "Rewrite the environment marker file"),
                    ("protect", "Toggle protection against destructive actions"),
                ],
            ),
            (
                "Snapshots",
                &[
                    ("snapshot create", "Capture a snapshot"),
                    ("snapshot list", "List snapshots"),
                    ("snapshot show", "Show one snapshot"),
                    ("snapshot restore", "Restore a snapshot"),
                    ("snapshot remove", "Delete a snapshot"),
                    ("snapshot prune", "Preview or prune older snapshots"),
                ],
            ),
            (
                "Portability",
                &[
                    ("export", "Export an environment archive"),
                    ("import", "Import an environment archive"),
                ],
            ),
        ],
        vec![
            format!("{cmd} env create demo --launcher stable"),
            format!("{cmd} env run demo -- status"),
            format!("{cmd} env snapshot create demo --label before-upgrade"),
        ],
        vec![
            format!("{cmd} help env create"),
            format!("{cmd} help env run"),
            format!("{cmd} help env snapshot"),
        ],
    )
}

pub fn env_snapshot_help(cmd: &str) -> String {
    render_group(
        "Environment snapshot commands",
        "Capture, inspect, restore, and prune point-in-time environment snapshots.",
        vec![
            format!("{cmd} env snapshot <command> [args]"),
            format!("{cmd} help env snapshot <command>"),
        ],
        &[(
            "Commands",
            &[
                ("create", "Capture a snapshot"),
                ("show", "Show one snapshot"),
                ("list", "List snapshots for one env or all envs"),
                ("restore", "Restore an environment from a snapshot"),
                ("remove", "Delete a snapshot"),
                ("prune", "Preview or remove older snapshots"),
            ],
        )],
        vec![
            format!("{cmd} env snapshot create demo --label before-upgrade"),
            format!("{cmd} env snapshot list demo"),
            format!("{cmd} env snapshot prune --all --older-than 30 --json"),
        ],
        vec![
            format!("{cmd} help env snapshot create"),
            format!("{cmd} help env snapshot restore"),
        ],
    )
}

pub fn launcher_help(cmd: &str) -> String {
    render_group(
        "Launcher commands",
        "Manage named command recipes for running OpenClaw and related workflows.",
        vec![
            format!("{cmd} launcher <command> [args]"),
            format!("{cmd} help launcher <command>"),
        ],
        &[(
            "Commands",
            &[
                ("add", "Create a launcher"),
                ("list", "List launchers"),
                ("show", "Show one launcher"),
                ("remove", "Remove a launcher"),
            ],
        )],
        vec![
            format!("{cmd} launcher add stable --command openclaw"),
            format!("{cmd} launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw"),
            format!("{cmd} launcher list"),
        ],
        vec![
            format!("{cmd} help launcher add"),
            format!("{cmd} help launcher list"),
        ],
    )
}

pub fn runtime_help(cmd: &str) -> String {
    render_group(
        "Runtime commands",
        "Register, install, verify, inspect, and update OpenClaw runtimes.",
        vec![
            format!("{cmd} runtime <command> [args]"),
            format!("{cmd} help runtime <command>"),
        ],
        &[
            (
                "Registry",
                &[
                    ("add", "Register an existing OpenClaw binary"),
                    ("list", "List runtimes"),
                    ("show", "Show one runtime"),
                    ("remove", "Remove a runtime"),
                    ("which", "Print the resolved binary path"),
                ],
            ),
            (
                "Install and update",
                &[
                    ("install", "Install a managed runtime"),
                    ("update", "Update one runtime or all runtimes"),
                    ("releases", "Inspect release entries from a manifest"),
                ],
            ),
            (
                "Health",
                &[("verify", "Verify one runtime or all runtimes")],
            ),
        ],
        vec![
            format!("{cmd} runtime add stable --path /path/to/openclaw"),
            format!("{cmd} runtime install nightly --url https://example.test/openclaw-nightly"),
            format!("{cmd} runtime update --all"),
        ],
        vec![
            format!("{cmd} help runtime install"),
            format!("{cmd} help runtime verify"),
        ],
    )
}

pub fn service_help(cmd: &str) -> String {
    render_group(
        "Service commands",
        "Inspect, install, operate, and migrate persistent OpenClaw services for environments.",
        vec![
            format!("{cmd} service <command> [args]"),
            format!("{cmd} help service <command>"),
        ],
        &[
            (
                "Inventory",
                &[
                    ("list", "List env-scoped service state"),
                    ("status", "Show one service or all services"),
                    ("discover", "Inventory discovered OpenClaw services"),
                    ("logs", "Read service logs"),
                ],
            ),
            (
                "Lifecycle",
                &[
                    ("install", "Install an env-scoped service"),
                    ("start", "Start a service"),
                    ("stop", "Stop a service"),
                    ("restart", "Restart a service"),
                    ("uninstall", "Remove a service"),
                ],
            ),
            (
                "Migration",
                &[
                    ("adopt-global", "Adopt the legacy global OpenClaw service"),
                    (
                        "restore-global",
                        "Restore the legacy global service from backup",
                    ),
                ],
            ),
        ],
        vec![
            format!("{cmd} service list"),
            format!("{cmd} service install demo"),
            format!("{cmd} service adopt-global demo --dry-run"),
        ],
        vec![
            format!("{cmd} help service install"),
            format!("{cmd} help service discover"),
        ],
    )
}

pub fn env_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "create" => render_leaf(
            "Create an environment",
            "Create an isolated OpenClaw environment and optionally bind a runtime or launcher.",
            vec![format!(
                "{cmd} env create <name> [--root <path>] [--port <port>] [--runtime <name>] [--launcher <name>] [--protect] [--json]"
            )],
            &[
                (
                    "--root <path>",
                    "Store the environment under a custom root path",
                ),
                (
                    "--port <port>",
                    "Persist an explicit gateway port in environment metadata",
                ),
                ("--runtime <name>", "Bind a runtime at creation time"),
                ("--launcher <name>", "Bind a launcher at creation time"),
                ("--protect", "Mark the environment as protected"),
                ("--json", "Print the created environment summary as JSON"),
            ],
            vec![
                format!("{cmd} env create demo --launcher stable"),
                format!("{cmd} env create nightly --runtime latest --port 19789"),
            ],
            &["Environments are the main isolation unit in OCM."],
        ),
        "clone" => render_leaf(
            "Clone an environment",
            "Copy an environment root and metadata into a new isolated environment.",
            vec![format!(
                "{cmd} env clone <source> <target> [--root <path>] [--json]"
            )],
            &[
                (
                    "--root <path>",
                    "Use a custom root path for the cloned environment",
                ),
                ("--json", "Print the cloned environment summary as JSON"),
            ],
            vec![format!("{cmd} env clone demo demo-copy")],
            &["Clone resets environment identity while preserving the copied state."],
        ),
        "export" => render_leaf(
            "Export an environment",
            "Write a portable environment archive that can be imported later.",
            vec![format!(
                "{cmd} env export <name> [--output <path>] [--json]"
            )],
            &[
                ("--output <path>", "Write the archive to a specific path"),
                ("--json", "Print the export summary as JSON"),
            ],
            vec![format!(
                "{cmd} env export demo --output ./backups/demo.ocm-env.tar"
            )],
            &[],
        ),
        "import" => render_leaf(
            "Import an environment",
            "Create a new environment from a portable environment archive.",
            vec![format!(
                "{cmd} env import <archive> [--name <name>] [--root <path>] [--json]"
            )],
            &[
                ("--name <name>", "Override the imported environment name"),
                ("--root <path>", "Override the imported environment root"),
                ("--json", "Print the imported environment summary as JSON"),
            ],
            vec![format!(
                "{cmd} env import ./backups/demo.ocm-env.tar --name restored-demo"
            )],
            &["Imported environments get a fresh identity and marker file."],
        ),
        "list" => render_leaf(
            "List environments",
            "Show all registered environments.",
            vec![format!("{cmd} env list [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print environment summaries as JSON"),
            ],
            vec![format!("{cmd} env list"), format!("{cmd} env list --json")],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "show" => render_leaf(
            "Show an environment",
            "Print stored metadata for one environment.",
            vec![format!("{cmd} env show <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the environment metadata as JSON"),
            ],
            vec![
                format!("{cmd} env show demo"),
                format!("{cmd} env show demo --raw"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "status" => render_leaf(
            "Show environment status",
            "Inspect the environment, its bindings, and related service state.",
            vec![format!("{cmd} env status <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the status summary as JSON"),
            ],
            vec![
                format!("{cmd} env status demo"),
                format!("{cmd} env status demo --raw"),
                format!("{cmd} env status demo --json"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "doctor" => render_leaf(
            "Inspect environment health",
            "Report environment problems without changing anything.",
            vec![format!("{cmd} env doctor <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print doctor findings as JSON"),
            ],
            vec![
                format!("{cmd} env doctor demo"),
                format!("{cmd} env doctor demo --raw"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "cleanup" => render_leaf(
            "Repair safe environment issues",
            "Preview or apply narrow, safe repairs such as marker rewrites and missing binding cleanup.",
            vec![format!(
                "{cmd} env cleanup (<name> | --all) [--yes] [--json]"
            )],
            &[
                (
                    "--all",
                    "Operate on every environment with actionable repairs",
                ),
                ("--yes", "Apply repairs instead of showing a preview"),
                ("--json", "Print cleanup summaries as JSON"),
            ],
            vec![
                format!("{cmd} env cleanup demo"),
                format!("{cmd} env cleanup demo --yes"),
                format!("{cmd} env cleanup --all --yes"),
            ],
            &["Only a narrow set of safe repairs is applied."],
        ),
        "repair-marker" => render_leaf(
            "Repair an environment marker",
            "Rewrite `.ocm-env.json` for a known environment root.",
            vec![format!("{cmd} env repair-marker <name> [--json]")],
            &[("--json", "Print the repair summary as JSON")],
            vec![format!("{cmd} env repair-marker demo")],
            &[],
        ),
        "use" => render_leaf(
            "Activate an environment in your shell",
            "Emit shell code that points the current shell at an environment.",
            vec![format!("{cmd} env use <name> [--shell zsh|bash|sh|fish]")],
            &[(
                "--shell zsh|bash|sh|fish",
                "Override the target shell when rendering activation",
            )],
            vec![
                format!("eval \"$({cmd} env use demo)\""),
                format!("{cmd} env use demo --shell zsh"),
            ],
            &["This command prints shell code. Use `eval` to apply it."],
        ),
        "exec" => render_leaf(
            "Run a command inside an environment",
            "Run any command with the target environment's OpenClaw variables injected.",
            vec![format!("{cmd} env exec <name> -- <command...>")],
            &[],
            vec![
                format!("{cmd} env exec demo -- env | rg OPENCLAW"),
                format!("{cmd} env exec demo -- openclaw status"),
            ],
            &["`--` is required before the command to execute."],
        ),
        "resolve" => render_leaf(
            "Show what an environment would run",
            "Resolve the runtime or launcher that would be used without executing it.",
            vec![format!(
                "{cmd} env resolve <name> [--runtime <name> | --launcher <name>] [--raw] [--json] [-- <openclaw args...>]"
            )],
            &[
                (
                    "--runtime <name>",
                    "Override the bound runtime for this resolution",
                ),
                (
                    "--launcher <name>",
                    "Override the bound launcher for this resolution",
                ),
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the resolution summary as JSON"),
            ],
            vec![
                format!("{cmd} env resolve demo"),
                format!("{cmd} env resolve demo --raw"),
                format!("{cmd} env resolve demo --launcher dev -- onboard"),
            ],
            &[
                "TTY output uses grouped cards by default. Piped output stays plain.",
                "Arguments after `--` are treated as OpenClaw arguments.",
            ],
        ),
        "run" => render_leaf(
            "Run OpenClaw inside an environment",
            "Resolve the runtime or launcher and execute OpenClaw inside the target environment.",
            vec![format!(
                "{cmd} env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>"
            )],
            &[
                (
                    "--runtime <name>",
                    "Override the bound runtime for this run",
                ),
                (
                    "--launcher <name>",
                    "Override the bound launcher for this run",
                ),
            ],
            vec![
                format!("{cmd} env run demo -- onboard"),
                format!("{cmd} env run demo -- status"),
                format!("{cmd} -- status"),
                format!("{cmd} @demo -- status"),
                format!("{cmd} env run demo --launcher dev -- gateway run"),
            ],
            &[
                "`--` is required before OpenClaw arguments.",
                "If an environment is active, you can also use the root-level `--` shortcut.",
                "For one-shot explicit env runs, use the root-level `@<env>` shortcut.",
            ],
        ),
        "set-runtime" => render_leaf(
            "Bind or clear a runtime",
            "Set the default runtime for an environment, or clear it with `none`.",
            vec![format!("{cmd} env set-runtime <name> <runtime|none>")],
            &[],
            vec![
                format!("{cmd} env set-runtime demo stable"),
                format!("{cmd} env set-runtime demo none"),
            ],
            &[],
        ),
        "set-launcher" => render_leaf(
            "Bind or clear a launcher",
            "Set the default launcher for an environment, or clear it with `none`.",
            vec![format!("{cmd} env set-launcher <name> <launcher|none>")],
            &[],
            vec![
                format!("{cmd} env set-launcher demo stable"),
                format!("{cmd} env set-launcher demo none"),
            ],
            &[],
        ),
        "protect" => render_leaf(
            "Toggle environment protection",
            "Mark an environment as protected or unprotected for destructive commands.",
            vec![format!("{cmd} env protect <name> <on|off>")],
            &[],
            vec![format!("{cmd} env protect demo on")],
            &[],
        ),
        "remove" | "rm" => render_leaf(
            "Remove an environment",
            "Delete an environment root and metadata, subject to safety rails.",
            vec![format!("{cmd} env remove <name> [--force]")],
            &[("--force", "Override protection for the target environment")],
            vec![
                format!("{cmd} env remove demo"),
                format!("{cmd} env remove demo --force"),
            ],
            &["Protected environments require `--force`."],
        ),
        "prune" => render_leaf(
            "Prune old environments",
            "Preview or remove unused environments older than a threshold.",
            vec![format!(
                "{cmd} env prune [--older-than <days>] [--yes] [--json]"
            )],
            &[
                (
                    "--older-than <days>",
                    "Age threshold in days. Defaults to 14",
                ),
                ("--yes", "Apply removals instead of showing a preview"),
                ("--json", "Print prune summaries as JSON"),
            ],
            vec![
                format!("{cmd} env prune"),
                format!("{cmd} env prune --older-than 30 --yes"),
            ],
            &[],
        ),
        "snapshot" => env_snapshot_help(cmd),
        _ => return None,
    })
}

pub fn env_snapshot_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "create" => render_leaf(
            "Create an environment snapshot",
            "Capture a point-in-time snapshot of an environment.",
            vec![format!(
                "{cmd} env snapshot create <name> [--label <label>] [--json]"
            )],
            &[
                ("--label <label>", "Add a human label to the snapshot"),
                ("--json", "Print the snapshot summary as JSON"),
            ],
            vec![format!(
                "{cmd} env snapshot create demo --label before-upgrade"
            )],
            &[],
        ),
        "show" => render_leaf(
            "Show one environment snapshot",
            "Print metadata for a single snapshot.",
            vec![format!(
                "{cmd} env snapshot show <name> <snapshot> [--json]"
            )],
            &[("--json", "Print the snapshot summary as JSON")],
            vec![format!("{cmd} env snapshot show demo 1742922000-123456789")],
            &[],
        ),
        "list" => render_leaf(
            "List environment snapshots",
            "List snapshots for one environment or for all environments.",
            vec![
                format!("{cmd} env snapshot list <name> [--json]"),
                format!("{cmd} env snapshot list --all [--json]"),
            ],
            &[("--json", "Print snapshot summaries as JSON")],
            vec![
                format!("{cmd} env snapshot list demo"),
                format!("{cmd} env snapshot list --all --json"),
            ],
            &[],
        ),
        "restore" => render_leaf(
            "Restore an environment snapshot",
            "Replace an environment root with the contents of a snapshot.",
            vec![format!(
                "{cmd} env snapshot restore <name> <snapshot> [--json]"
            )],
            &[("--json", "Print the restore summary as JSON")],
            vec![format!(
                "{cmd} env snapshot restore demo 1742922000-123456789"
            )],
            &["Snapshot restore keeps existing safety rails around foreign directories."],
        ),
        "remove" => render_leaf(
            "Remove an environment snapshot",
            "Delete snapshot metadata and archived content for a snapshot.",
            vec![format!(
                "{cmd} env snapshot remove <name> <snapshot> [--json]"
            )],
            &[("--json", "Print the removal summary as JSON")],
            vec![format!(
                "{cmd} env snapshot remove demo 1742922000-123456789"
            )],
            &[],
        ),
        "prune" => render_leaf(
            "Prune environment snapshots",
            "Preview or remove older snapshots for one environment or all environments.",
            vec![format!(
                "{cmd} env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--yes] [--json]"
            )],
            &[
                ("--all", "Operate on snapshots across all environments"),
                (
                    "--keep <count>",
                    "Keep this many recent snapshots per scope",
                ),
                (
                    "--older-than <days>",
                    "Only consider snapshots older than this many days",
                ),
                ("--yes", "Apply removals instead of showing a preview"),
                ("--json", "Print prune summaries as JSON"),
            ],
            vec![
                format!("{cmd} env snapshot prune demo --keep 5 --yes"),
                format!("{cmd} env snapshot prune --all --older-than 30 --json"),
            ],
            &[],
        ),
        _ => return None,
    })
}

pub fn launcher_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "add" => render_leaf(
            "Create a launcher",
            "Register a named command recipe for running OpenClaw or related workflows.",
            vec![format!(
                "{cmd} launcher add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>] [--json]"
            )],
            &[
                (
                    "--command <launcher>",
                    "Shell command used when the launcher runs",
                ),
                (
                    "--cwd <path>",
                    "Optional working directory for the launcher",
                ),
                ("--description <text>", "Optional human description"),
                ("--json", "Print the launcher record as JSON"),
            ],
            vec![
                format!("{cmd} launcher add stable --command openclaw"),
                format!("{cmd} launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw"),
            ],
            &[],
        ),
        "list" => render_leaf(
            "List launchers",
            "Show all registered launchers.",
            vec![format!("{cmd} launcher list [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print launchers as JSON"),
            ],
            vec![format!("{cmd} launcher list")],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "show" => render_leaf(
            "Show a launcher",
            "Print one launcher definition.",
            vec![format!("{cmd} launcher show <name> [--json]")],
            &[("--json", "Print the launcher as JSON")],
            vec![format!("{cmd} launcher show stable")],
            &[],
        ),
        "remove" | "rm" => render_leaf(
            "Remove a launcher",
            "Delete a launcher definition.",
            vec![format!("{cmd} launcher remove <name>")],
            &[],
            vec![format!("{cmd} launcher remove stable")],
            &[],
        ),
        _ => return None,
    })
}

pub fn runtime_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "add" => render_leaf(
            "Register an existing runtime",
            "Register a named OpenClaw binary that already exists on disk.",
            vec![format!(
                "{cmd} runtime add <name> --path <binary> [--description <text>] [--json]"
            )],
            &[
                ("--path <binary>", "Filesystem path to the OpenClaw binary"),
                ("--description <text>", "Optional human description"),
                ("--json", "Print the runtime record as JSON"),
            ],
            vec![format!("{cmd} runtime add stable --path /path/to/openclaw")],
            &[],
        ),
        "install" => render_leaf(
            "Install a managed runtime",
            "Install a runtime from a local binary, a direct URL, or a release manifest.",
            vec![format!(
                "{cmd} runtime install <name> (--path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force] [--json]"
            )],
            &[
                ("--path <binary>", "Install from a local binary path"),
                ("--url <url>", "Install from a direct binary URL"),
                (
                    "--manifest-url <url>",
                    "Use a release manifest as the install source",
                ),
                (
                    "--version <version>",
                    "Pick a manifest release by explicit version",
                ),
                ("--channel <channel>", "Pick a manifest release by channel"),
                ("--description <text>", "Optional human description"),
                (
                    "--force",
                    "Replace an existing managed runtime of the same name",
                ),
                ("--json", "Print the runtime record as JSON"),
            ],
            vec![
                format!("{cmd} runtime install managed-stable --path ./target/debug/openclaw"),
                format!(
                    "{cmd} runtime install nightly --url https://example.test/openclaw-nightly"
                ),
                format!(
                    "{cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable"
                ),
            ],
            &["Exactly one install source must be provided."],
        ),
        "update" => render_leaf(
            "Update managed runtimes",
            "Update one runtime or every managed runtime using stored release provenance.",
            vec![format!(
                "{cmd} runtime update (<name> | --all) [--version <version> | --channel <channel>] [--json]"
            )],
            &[
                ("--all", "Update every managed runtime"),
                (
                    "--version <version>",
                    "Override the selected release version",
                ),
                (
                    "--channel <channel>",
                    "Override the selected release channel",
                ),
                ("--json", "Print update summaries as JSON"),
            ],
            vec![
                format!("{cmd} runtime update stable"),
                format!("{cmd} runtime update stable --version 0.3.0"),
                format!("{cmd} runtime update --all"),
            ],
            &[],
        ),
        "releases" => render_leaf(
            "Inspect release manifest entries",
            "Show releases resolved from a remote manifest without installing them.",
            vec![format!(
                "{cmd} runtime releases --manifest-url <url> [--version <version> | --channel <channel>] [--json]"
            )],
            &[
                ("--manifest-url <url>", "Manifest URL to inspect"),
                (
                    "--version <version>",
                    "Select one release by explicit version",
                ),
                ("--channel <channel>", "Select one release by channel"),
                ("--json", "Print releases as JSON"),
            ],
            vec![
                format!(
                    "{cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --channel stable"
                ),
                format!(
                    "{cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --version 0.2.0 --json"
                ),
            ],
            &[],
        ),
        "list" => render_leaf(
            "List runtimes",
            "Show registered and managed runtimes.",
            vec![format!("{cmd} runtime list [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print runtimes as JSON"),
            ],
            vec![
                format!("{cmd} runtime list"),
                format!("{cmd} runtime list --json"),
            ],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "show" => render_leaf(
            "Show a runtime",
            "Print one runtime record.",
            vec![format!("{cmd} runtime show <name> [--json]")],
            &[("--json", "Print the runtime record as JSON")],
            vec![format!("{cmd} runtime show stable")],
            &[],
        ),
        "verify" => render_leaf(
            "Verify runtimes",
            "Check runtime health for one runtime or every runtime.",
            vec![format!("{cmd} runtime verify (<name> | --all) [--json]")],
            &[
                ("--all", "Verify every runtime"),
                ("--json", "Print verification summaries as JSON"),
            ],
            vec![
                format!("{cmd} runtime verify stable"),
                format!("{cmd} runtime verify --all"),
            ],
            &[],
        ),
        "which" => render_leaf(
            "Print a runtime binary path",
            "Show the resolved binary path for a runtime.",
            vec![format!("{cmd} runtime which <name> [--json]")],
            &[("--json", "Print the resolution summary as JSON")],
            vec![format!("{cmd} runtime which stable")],
            &[],
        ),
        "remove" | "rm" => render_leaf(
            "Remove a runtime",
            "Delete a runtime record.",
            vec![format!("{cmd} runtime remove <name>")],
            &[],
            vec![format!("{cmd} runtime remove stable")],
            &[],
        ),
        _ => return None,
    })
}

pub fn service_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "discover" => render_leaf(
            "Discover OpenClaw services",
            "Inventory OCM-managed, legacy, and foreign OpenClaw services on the current machine.",
            vec![format!("{cmd} service discover [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print discovered services as JSON"),
            ],
            vec![
                format!("{cmd} service discover"),
                format!("{cmd} service discover --json"),
            ],
            &[],
        ),
        "adopt-global" => render_leaf(
            "Adopt the legacy global service",
            "Move the legacy global OpenClaw LaunchAgent into the env-scoped OCM service model.",
            vec![format!(
                "{cmd} service adopt-global <env> [--dry-run] [--json]"
            )],
            &[
                (
                    "--dry-run",
                    "Preview adoption without mutating files or launchd state",
                ),
                ("--json", "Print the adoption summary as JSON"),
            ],
            vec![format!("{cmd} service adopt-global demo --dry-run")],
            &["Adoption is intentionally conservative and only targets the legacy global label."],
        ),
        "restore-global" => render_leaf(
            "Restore the legacy global service",
            "Restore a previously adopted global OpenClaw service from backup.",
            vec![format!(
                "{cmd} service restore-global <env> [--dry-run] [--json]"
            )],
            &[
                (
                    "--dry-run",
                    "Preview the restore without mutating files or launchd state",
                ),
                ("--json", "Print the restore summary as JSON"),
            ],
            vec![format!("{cmd} service restore-global demo --dry-run")],
            &[],
        ),
        "install" => render_leaf(
            "Install an env-scoped service",
            "Create a persistent service for an environment using the current binding and effective port.",
            vec![format!("{cmd} service install <env> [--json]")],
            &[("--json", "Print the install summary as JSON")],
            vec![format!("{cmd} service install demo")],
            &["If the preferred port is busy, OCM auto-provisions the next free port and warns."],
        ),
        "list" => render_leaf(
            "List env-scoped services",
            "Show service state for every known environment.",
            vec![format!("{cmd} service list [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print service summaries as JSON"),
            ],
            vec![format!("{cmd} service list")],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "status" => render_leaf(
            "Show service status",
            "Inspect one environment service or every environment service.",
            vec![
                format!("{cmd} service status <env> [--raw] [--json]"),
                format!("{cmd} service status --all [--raw] [--json]"),
            ],
            &[
                ("--raw", "Force plain output instead of TTY cards or tables"),
                ("--all", "Show every environment service"),
                ("--json", "Print service summaries as JSON"),
            ],
            vec![
                format!("{cmd} service status demo"),
                format!("{cmd} service status demo --raw"),
                format!("{cmd} service status --all"),
            ],
            &["TTY output uses cards for one env and a table for `--all` by default."],
        ),
        "logs" => render_leaf(
            "Read service logs",
            "Print service stdout or stderr logs from the environment root.",
            vec![format!(
                "{cmd} service logs <env> [--stderr] [--tail <count>] [--json]"
            )],
            &[
                ("--stderr", "Read stderr instead of stdout"),
                ("--tail <count>", "Only print the last N lines"),
                ("--json", "Print log metadata and content as JSON"),
            ],
            vec![
                format!("{cmd} service logs demo"),
                format!("{cmd} service logs demo --stderr --tail 50"),
            ],
            &["Plain-text output is intentionally raw so it can be piped directly."],
        ),
        "start" => render_leaf(
            "Start a service",
            "Start an installed env-scoped service.",
            vec![format!("{cmd} service start <env> [--json]")],
            &[("--json", "Print the action summary as JSON")],
            vec![format!("{cmd} service start demo")],
            &[],
        ),
        "stop" => render_leaf(
            "Stop a service",
            "Stop an installed env-scoped service.",
            vec![format!("{cmd} service stop <env> [--json]")],
            &[("--json", "Print the action summary as JSON")],
            vec![format!("{cmd} service stop demo")],
            &[],
        ),
        "restart" => render_leaf(
            "Restart a service",
            "Restart an installed env-scoped service.",
            vec![format!("{cmd} service restart <env> [--json]")],
            &[("--json", "Print the action summary as JSON")],
            vec![format!("{cmd} service restart demo")],
            &[],
        ),
        "uninstall" => render_leaf(
            "Uninstall a service",
            "Remove an env-scoped service definition.",
            vec![format!("{cmd} service uninstall <env> [--json]")],
            &[("--json", "Print the action summary as JSON")],
            vec![format!("{cmd} service uninstall demo")],
            &[],
        ),
        _ => return None,
    })
}
