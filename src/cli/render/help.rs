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
        format!("OpenClaw Manager v{}", env!("CARGO_PKG_VERSION")),
        String::new(),
        "Manage isolated OpenClaw environments, releases, runtimes, launchers, and env supervision."
            .to_string(),
    ];
    let mut lines = lines;
    push_section(
        &mut lines,
        "Usage",
        format_usage(&[
            format!("{cmd} [--color <mode>] <command> [args]"),
            format!("{cmd} help <command>"),
        ]),
    );
    push_section(
        &mut lines,
        "Global options",
        format_entries(&[(
            "--color <mode>",
            "Color policy for pretty output: auto, always, or never",
        )]),
    );
    push_section(
        &mut lines,
        "Commands",
        format_entries(&[
            ("setup", "Guided setup for release and local-dev flows"),
            (
                "dev",
                "OpenClaw development envs with worktrees and watch mode",
            ),
            (
                "start",
                "Fast path: create or reuse an env and keep it running",
            ),
            (
                "upgrade",
                "Update one env or all envs and restart services when needed",
            ),
            (
                "doctor",
                "Check host software for release and feature readiness",
            ),
            ("self", "Update the installed ocm binary"),
            (
                "env",
                "Environment lifecycle, binding, execution, snapshots, and repair",
            ),
            ("migrate", "Bring an existing plain OpenClaw home into OCM"),
            (
                "adopt",
                "Inspect and control the explicit OpenClaw adoption flow",
            ),
            ("release", "Published OpenClaw releases and release details"),
            ("launcher", "Named command recipes for running OpenClaw"),
            (
                "runtime",
                "Registered and installer-managed OpenClaw runtimes",
            ),
            (
                "logs",
                "Tail env gateway logs from the env log files or service fallback logs",
            ),
            (
                "service",
                "Supervise env gateways through the OCM background service",
            ),
            ("init", "Shell setup snippets for using ocm"),
            ("help", "Show help for a command or command group"),
            ("--version", "Show the installed ocm version"),
        ]),
    );
    push_section(
        &mut lines,
        "Get started",
        format_examples(&[
            format!("{cmd} dev shaks"),
            format!("{cmd} dev shaks --watch"),
            format!("{cmd} start mira"),
            format!("{cmd} migrate mira"),
            format!("{cmd} adopt inspect"),
            format!("{cmd} @mira -- onboard"),
            format!("{cmd} @mira -- status"),
            format!("{cmd} upgrade mira"),
            format!("{cmd} start mira --channel beta"),
            format!(
                "{cmd} start luna --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service"
            ),
        ]),
    );
    push_section(
        &mut lines,
        "More",
        format_examples(&[
            format!("{cmd} help setup"),
            format!("{cmd} help dev"),
            format!("{cmd} help start"),
            format!("{cmd} help migrate"),
            format!("{cmd} help adopt"),
            format!("{cmd} help upgrade"),
            format!("{cmd} help doctor"),
            format!("{cmd} doctor host"),
            format!("{cmd} help self"),
            format!("{cmd} help env"),
            format!("{cmd} help release"),
            format!("{cmd} help logs"),
            format!("{cmd} help service"),
            format!("{cmd} help runtime install"),
            format!("{cmd} --color always env list"),
        ]),
    );
    finish(lines)
}

pub fn setup_help(cmd: &str) -> String {
    render_leaf(
        "Guided setup",
        "Interactive setup for stable, beta, specific-version, or local-checkout OpenClaw environments.",
        vec![format!("{cmd} setup")],
        &[],
        vec![format!("{cmd} setup")],
        &[
            "Setup asks a few questions, then runs the same env-first flow as `start`.",
            "Official release choices prefer host Node.js >= 22.14.0 and npm, and OCM can manage a private copy on supported platforms when they are missing.",
            "If git is missing, setup can offer to install it for repo-aware coding workflows.",
            "When run inside an OpenClaw checkout, local mode defaults to `pnpm openclaw` in that folder.",
            "If OCM detects an existing plain OpenClaw home, setup points you at `migrate` so you can bring that state under OCM instead of starting fresh.",
            "Use `start` when you already know the source you want.",
        ],
    )
}

pub fn logs_help(cmd: &str) -> String {
    render_leaf(
        "Read env logs",
        "Tail one env log with follow support. OCM chooses the active log file between the env's OpenClaw gateway logs and the OCM background service child logs.",
        vec![format!(
            "{cmd} logs <env> [--stderr | --all-streams] [--tail <count>] [--follow] [--raw] [--json]"
        )],
        &[
            ("--stderr", "Read stderr instead of stdout"),
            (
                "--all-streams",
                "Read stdout and stderr together in time order",
            ),
            ("--tail <count>", "Print the last N lines before streaming"),
            ("--follow", "Keep following the log file like tail -f"),
            ("--raw", "Print log content without the TTY header"),
            ("--json", "Print log metadata and snapshot content as JSON"),
        ],
        vec![
            format!("{cmd} logs mira"),
            format!("{cmd} logs mira --follow"),
            format!("{cmd} logs mira --all-streams --follow"),
            format!("{cmd} logs mira --stderr --tail 100"),
        ],
        &[
            "Default output shows the last 50 lines.",
            "Follow mode cannot be combined with --json.",
            "When both env and service logs exist, OCM reads the newer active file.",
            "Foreground runs still work because OCM can still read the env's own gateway log files directly.",
        ],
    )
}

pub fn dev_help(cmd: &str) -> String {
    render_group(
        "Development envs",
        "Provision OpenClaw dev envs from a checkout worktree, bootstrap the minimum local config, and run the gateway in the foreground. If OCM cannot infer the repo on the first run, it asks once and reuses that repo for later dev envs.",
        vec![format!(
            "{cmd} dev <env> [--repo <path>] [--root <path>] [--port <port>] [--watch] [--onboard]"
        )],
        &[(
            "Commands",
            &[("status", "Show one dev env or all dev envs")],
        )],
        vec![
            format!("{cmd} dev shaks"),
            format!("{cmd} dev shaks --root /tmp/shaks"),
            format!("{cmd} dev shaks --watch"),
            format!("{cmd} dev shaks --onboard"),
            format!("{cmd} dev status"),
            format!("{cmd} dev status shaks --json"),
        ],
        vec![format!("{cmd} help dev status")],
    )
}

pub fn dev_command_help(cmd: &str, action: &str) -> Option<String> {
    match action {
        "status" => Some(render_leaf(
            "Show dev env status",
            "List dev envs or inspect one dev env, including the repo checkout, worktree path, and effective gateway port.",
            vec![format!("{cmd} dev status [env] [--raw] [--json]")],
            &[
                ("[env]", "Optional env name"),
                ("--raw", "Print machine-friendly key/value output"),
                ("--json", "Print JSON output"),
            ],
            vec![
                format!("{cmd} dev status"),
                format!("{cmd} dev status shaks"),
                format!("{cmd} dev status shaks --json"),
            ],
            &[
                "Only envs created through `ocm dev` appear here.",
                "Use `ocm dev <env>` to create or reuse a dev env and start its gateway.",
            ],
        )),
        _ => None,
    }
}

pub fn migrate_help(cmd: &str) -> String {
    render_leaf(
        "Migrate an existing OpenClaw home",
        "Create a managed env from an existing plain OpenClaw home in one step, keeping your durable user state and rewriting it for the new OCM-managed root.",
        vec![format!(
            "{cmd} migrate <env> [<source-home>] [--root <path>] [--raw] [--json]"
        )],
        &[
            ("<env>", "Target env name OCM should create"),
            (
                "[source-home]",
                "Optional explicit .openclaw home path to import",
            ),
            ("--root <path>", "Optional explicit target env root"),
            ("--raw", "Print machine-friendly key/value output"),
            ("--json", "Print JSON output"),
        ],
        vec![
            format!("{cmd} migrate mira"),
            format!("{cmd} migrate mira /path/to/.openclaw"),
            format!("{cmd} migrate --name mira --json"),
        ],
        &[
            "Without an explicit source path, OCM imports from the default plain OpenClaw home under the current user home.",
            "Migrate preserves config, auth, sessions, logs, and other durable user state, rewrites env-scoped paths for the new managed root, and clears only live runtime residue like locks, pid files, and sockets.",
            "If `openclaw` is already available on PATH, migrate also binds the imported env to an env-local migrated launcher so you can keep going through OCM immediately.",
            "Use `adopt inspect` or `adopt plan` if you want read-only preview commands before importing.",
        ],
    )
}

pub fn adopt_help(cmd: &str) -> String {
    render_group(
        "Adoption commands",
        "Inspect and control the explicit flow for bringing an existing plain OpenClaw home into an OCM-managed environment.",
        vec![format!("{cmd} adopt <command> [args]")],
        &[(
            "Commands",
            &[
                ("import", "Import a plain OpenClaw home into a managed env"),
                ("inspect", "Show what plain OpenClaw home OCM would inspect"),
                ("plan", "Show the target env and root a migration would use"),
            ],
        )],
        vec![
            format!("{cmd} adopt inspect"),
            format!("{cmd} adopt plan --name mira"),
            format!("{cmd} adopt import --name mira"),
            format!("{cmd} adopt inspect /path/to/.openclaw"),
        ],
        vec![
            format!("{cmd} help adopt import"),
            format!("{cmd} help adopt inspect"),
            format!("{cmd} help adopt plan"),
        ],
    )
}

pub fn adopt_command_help(cmd: &str, action: &str) -> Option<String> {
    match action {
        "import" => Some(render_leaf(
            "Import a plain OpenClaw home",
            "Create a managed env from a plain OpenClaw home, preserve config, auth, sessions, and logs, and clear only live runtime residue like locks, pid files, and sockets.",
            vec![format!(
                "{cmd} adopt import --name <env> [<source-home>] [--root <path>] [--raw] [--json]"
            )],
            &[
                ("--name <env>", "Target env name OCM should create"),
                (
                    "[source-home]",
                    "Optional explicit .openclaw home path to import",
                ),
                ("--root <path>", "Optional explicit target env root"),
                ("--raw", "Print machine-friendly key/value output"),
                ("--json", "Print JSON output"),
            ],
            vec![
                format!("{cmd} adopt import --name mira"),
                format!("{cmd} adopt import --name mira /path/to/.openclaw"),
                format!("{cmd} adopt import --name mira --root /tmp/mira"),
            ],
            &[
                "Without an explicit source path, OCM imports from the default plain OpenClaw home under the current user home.",
                "This creates a managed env and rewrites env-scoped OpenClaw paths for the new target.",
                "If `openclaw` is already available on PATH, import also binds the env to an env-local migrated launcher so it is immediately runnable through OCM.",
            ],
        )),
        "inspect" => Some(render_leaf(
            "Inspect a migration source",
            "Report the plain OpenClaw home OCM would inspect before any import or migration work happens.",
            vec![format!(
                "{cmd} adopt inspect [<source-home>] [--raw] [--json]"
            )],
            &[
                (
                    "[source-home]",
                    "Optional explicit .openclaw home path to inspect",
                ),
                ("--raw", "Print machine-friendly key/value output"),
                ("--json", "Print JSON output"),
            ],
            vec![
                format!("{cmd} adopt inspect"),
                format!("{cmd} adopt inspect /path/to/.openclaw"),
                format!("{cmd} adopt inspect --json"),
            ],
            &[
                "Without an explicit path, OCM inspects the default plain OpenClaw home under the current user home.",
                "This command is read-only. It does not create, import, or modify any env.",
            ],
        )),
        "plan" => Some(render_leaf(
            "Plan a migration target",
            "Show the env name and target root OCM would use for a migration without creating or importing anything.",
            vec![format!(
                "{cmd} adopt plan --name <env> [<source-home>] [--root <path>] [--raw] [--json]"
            )],
            &[
                ("--name <env>", "Target env name OCM would create or update"),
                (
                    "[source-home]",
                    "Optional explicit .openclaw home path to inspect",
                ),
                ("--root <path>", "Optional explicit target env root"),
                ("--raw", "Print machine-friendly key/value output"),
                ("--json", "Print JSON output"),
            ],
            vec![
                format!("{cmd} adopt plan --name mira"),
                format!("{cmd} adopt plan --name mira /path/to/.openclaw"),
                format!("{cmd} adopt plan --name mira --root /tmp/mira"),
            ],
            &[
                "Without an explicit source path, OCM plans from the default plain OpenClaw home under the current user home.",
                "This command is read-only. It does not create, import, or modify any env.",
            ],
        )),
        _ => None,
    }
}

pub fn start_help(cmd: &str) -> String {
    render_leaf(
        "Start an environment",
        "Fast path: create or reuse an environment, prepare the selected OpenClaw source, start its background service, and optionally run onboarding.",
        vec![format!(
            "{cmd} start [name] [--runtime <name> | --launcher <name> | --version <version> | --channel <channel> | --command <command>] [--cwd <path>] [--root <path>] [--port <port>] [--protect] [--service | --no-service] [--onboard | --no-onboard] [--json]"
        )],
        &[
            (
                "[name]",
                "Optional environment name. If omitted, ocm generates a new one.",
            ),
            ("--runtime <name>", "Use one installed runtime by name"),
            ("--launcher <name>", "Use one existing launcher by name"),
            (
                "--version <version>",
                "Install or reuse one exact published OpenClaw release",
            ),
            (
                "--channel <channel>",
                "Install or reuse the published release currently tagged for one channel",
            ),
            (
                "--command <command>",
                "Create or reuse an env-local launcher from a local command",
            ),
            ("--cwd <path>", "Working directory for `--command`"),
            (
                "--root <path>",
                "Custom root for a newly created environment",
            ),
            (
                "--port <port>",
                "Persist an explicit gateway port for a new environment",
            ),
            ("--protect", "Mark the environment as protected"),
            (
                "--service",
                "Keep the default background-service behavior explicit",
            ),
            (
                "--no-service",
                "Skip installing and starting a background service",
            ),
            (
                "--onboard",
                "Run onboarding even when the env already exists",
            ),
            (
                "--no-onboard",
                "Skip onboarding output and print next steps instead",
            ),
            ("--json", "Print a machine-readable start summary"),
        ],
        vec![
            format!("{cmd} start"),
            format!("{cmd} start mira --channel stable"),
            format!("{cmd} start rowan --version 2026.3.24"),
            format!(
                "{cmd} start luna --command 'pnpm openclaw' --cwd /path/to/openclaw --no-onboard"
            ),
            format!(
                "{cmd} start luna --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service --no-onboard"
            ),
        ],
        &[
            "If an environment already exists, start reuses it and only adjusts binding/protection when you asked for it.",
            "Start installs and starts the env service by default. Use `--no-service` when you do not want a background process.",
            "Managed services currently support launchd on macOS and systemd --user on Linux.",
            "Official release selectors prefer host Node.js >= 22.14.0 and npm, and OCM can manage a private copy on supported platforms when they are missing.",
            "When start creates a new official-release env interactively, it can offer to install git for repo-aware coding workflows.",
            "If OCM detects an existing plain OpenClaw home, start keeps the new env fresh and points you at `migrate` if you want to bring that older state under OCM.",
            "`--json` requires `--no-onboard` because onboarding is interactive.",
        ],
    )
}

pub fn upgrade_help(cmd: &str) -> String {
    render_leaf(
        "Upgrade environments",
        "Update OpenClaw for one environment or every environment, and refresh running services when needed.",
        vec![
            format!(
                "{cmd} upgrade <env> [--version <version> | --channel <channel>] [--raw] [--json]"
            ),
            format!("{cmd} upgrade --all [--raw] [--json]"),
        ],
        &[
            (
                "--version <version>",
                "Move one env to one exact published release",
            ),
            (
                "--channel <channel>",
                "Move one env to the release for one channel",
            ),
            ("--all", "Upgrade every env that can be updated safely"),
            ("--raw", "Force plain output instead of TTY cards or tables"),
            ("--json", "Print upgrade summaries as JSON"),
        ],
        vec![
            format!("{cmd} upgrade mira"),
            format!("{cmd} upgrade mira --channel beta"),
            format!("{cmd} upgrade mira --version 2026.3.24"),
            format!("{cmd} upgrade --all"),
        ],
        &[
            "Channel-tracked runtimes move forward automatically.",
            "Pinned runtimes stay pinned unless you pass --version or --channel explicitly.",
            "Local-command environments are reported clearly instead of being changed behind your back.",
        ],
    )
}

pub fn doctor_help(cmd: &str) -> String {
    render_group(
        "Doctor commands",
        "Inspect host-level prerequisites and common software that OpenClaw workflows rely on.",
        vec![format!("{cmd} doctor <command>")],
        &[(
            "Commands",
            &[(
                "host",
                "Check required software for official releases and common optional tools",
            )],
        )],
        vec![format!("{cmd} doctor host")],
        vec![format!("{cmd} help doctor host")],
    )
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

pub fn self_help(cmd: &str) -> String {
    render_group(
        "Self commands",
        "Inspect and update the installed ocm binary.",
        vec![
            format!("{cmd} self <command> [args]"),
            format!("{cmd} help self <command>"),
        ],
        &[(
            "Maintenance",
            &[("update", "Check for or install a newer ocm release")],
        )],
        vec![
            format!("{cmd} self update --check"),
            format!("{cmd} self update"),
            format!("{cmd} self update --version {}", env!("CARGO_PKG_VERSION")),
        ],
        vec![format!("{cmd} help self update")],
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
                    ("destroy", "Preview or remove an env and its OCM service"),
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
            format!("{cmd} env create mira --launcher stable"),
            format!("{cmd} env run mira -- status"),
            format!("{cmd} env snapshot create mira --label before-upgrade"),
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
            format!("{cmd} env snapshot create mira --label before-upgrade"),
            format!("{cmd} env snapshot list mira"),
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

pub fn release_help(cmd: &str) -> String {
    render_group(
        "Release commands",
        "Inspect published OpenClaw releases before installing them as local runtimes.",
        vec![
            format!("{cmd} release <command> [args]"),
            format!("{cmd} help release <command>"),
        ],
        &[(
            "Commands",
            &[
                (
                    "install",
                    "Install a published OpenClaw release as a runtime",
                ),
                ("list", "List published OpenClaw releases"),
                ("show", "Show one published OpenClaw release"),
            ],
        )],
        vec![
            format!("{cmd} release list"),
            format!("{cmd} release list --channel stable"),
            format!("{cmd} release install --channel stable"),
            format!("{cmd} release show 2026.3.24"),
            format!("{cmd} release show --channel stable"),
        ],
        vec![
            format!("{cmd} help release install"),
            format!("{cmd} help release list"),
            format!("{cmd} help release show"),
        ],
    )
}

pub fn self_command_help(cmd: &str, action: &str) -> Option<String> {
    match action {
        "update" => Some(render_leaf(
            "Update ocm",
            "Check for or install a newer ocm release in place.",
            vec![format!(
                "{cmd} self update [--version <version>] [--check] [--raw] [--json]"
            )],
            &[
                (
                    "--version <version>",
                    "Install one exact ocm release tag or version",
                ),
                ("--check", "Only report whether an update is available"),
                ("--raw", "Use plain text instead of pretty TTY cards"),
                ("--json", "Print the update summary as JSON"),
            ],
            vec![
                format!("{cmd} self update --check"),
                format!("{cmd} self update"),
                format!("{cmd} self update --version {}", env!("CARGO_PKG_VERSION")),
            ],
            &[
                "The current binary is replaced in place on supported macOS and Linux installs.",
                "Exact versions accept either `1.2.3` or `v1.2.3`.",
            ],
        )),
        _ => None,
    }
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
                    (
                        "install",
                        "Install a managed runtime from OpenClaw releases or a custom source",
                    ),
                    ("update", "Update one runtime or all runtimes"),
                    (
                        "releases",
                        "Inspect release entries from the official source or a manifest",
                    ),
                ],
            ),
            (
                "Health",
                &[("verify", "Verify one runtime or all runtimes")],
            ),
        ],
        vec![
            format!("{cmd} runtime add stable --path /path/to/openclaw"),
            format!("{cmd} runtime install --channel stable"),
            format!("{cmd} runtime update --all"),
        ],
        vec![
            format!("{cmd} help release"),
            format!("{cmd} help runtime install"),
            format!("{cmd} help runtime verify"),
        ],
    )
}

pub fn doctor_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "host" => render_leaf(
            "Check host readiness",
            "Show required software for official release installs, plus recommended tools for common OpenClaw features and local workflows.",
            vec![
                format!("{cmd} doctor host [--raw] [--json]"),
                format!("{cmd} doctor host --fix git --yes [--json]"),
            ],
            &[
                (
                    "--raw",
                    "Force plain host-check output instead of TTY card rendering",
                ),
                ("--json", "Print the host-check summary as JSON"),
                ("--fix <tool>", "Install one supported host tool"),
                ("--yes", "Allow host changes when used with `--fix`"),
            ],
            vec![
                format!("{cmd} doctor host"),
                format!("{cmd} doctor host --fix git --yes"),
            ],
            &[
                "Official release installs prefer host Node.js >= 22.14.0 and npm.",
                "On supported platforms, OCM can manage a private copy when they are missing.",
                "Git is the first supported host fix target; OCM will not install Homebrew automatically.",
                "Recommended tools are advisory; they do not block local-command or launcher flows.",
            ],
        ),
        _ => return None,
    })
}

pub fn service_help(cmd: &str) -> String {
    render_group(
        "Service commands",
        "Manage env gateway supervision through the single OCM background service.",
        vec![
            format!("{cmd} service <command> [args]"),
            format!("{cmd} help service <command>"),
        ],
        &[
            (
                "Inspect",
                &[("status", "Show one env gateway or all supervised envs")],
            ),
            (
                "Lifecycle",
                &[
                    (
                        "install",
                        "Enable an env in the background service without starting it yet",
                    ),
                    (
                        "start",
                        "Start one env gateway under the background service",
                    ),
                    ("stop", "Stop one env gateway without disabling it"),
                    (
                        "restart",
                        "Restart one env gateway under the background service",
                    ),
                    ("uninstall", "Disable one env in the background service"),
                ],
            ),
        ],
        vec![
            format!("{cmd} service status"),
            format!("{cmd} service install mira"),
            format!("{cmd} service start mira"),
        ],
        vec![
            format!("{cmd} help service install"),
            format!("{cmd} help service status"),
        ],
    )
}

pub fn env_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "create" => render_leaf(
            "Create an environment",
            "Create an isolated OpenClaw environment and optionally bind a runtime, install an official OpenClaw release, or bind a launcher.",
            vec![format!(
                "{cmd} env create <name> [--root <path>] [--port <port>] [--runtime <name> | --version <version> | --channel <channel>] [--launcher <name>] [--protect] [--raw] [--json]"
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
                (
                    "--version <version>",
                    "Install or reuse one exact published OpenClaw release and bind it",
                ),
                (
                    "--channel <channel>",
                    "Install or reuse the published release currently tagged for one channel",
                ),
                ("--launcher <name>", "Bind a launcher at creation time"),
                ("--protect", "Mark the environment as protected"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the created environment summary as JSON"),
            ],
            vec![
                format!("{cmd} env create mira --launcher stable"),
                format!("{cmd} env create rowan --channel stable"),
                format!("{cmd} env create ember --version 2026.3.24"),
            ],
            &[
                "Environments are the main isolation unit in OCM.",
                "Computed gateway ports reserve the full local OpenClaw port family and skip the machine-wide OpenClaw config when present.",
                "Use exactly one of `--runtime`, `--version`, or `--channel`.",
            ],
        ),
        "clone" => render_leaf(
            "Clone an environment",
            "Copy an environment root and metadata into a new isolated environment.",
            vec![format!(
                "{cmd} env clone <source> <target> [--root <path>] [--raw] [--json]"
            )],
            &[
                (
                    "--root <path>",
                    "Use a custom root path for the cloned environment",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the cloned environment summary as JSON"),
            ],
            vec![format!("{cmd} env clone mira rowan")],
            &[
                "Clone resets environment identity while preserving the copied workspace and env config.",
                "Clone assigns a fresh gateway port to the new env to avoid collisions.",
                "Computed gateway ports reserve the full local OpenClaw port family and skip the machine-wide OpenClaw config when present.",
                "Clone rewrites env-scoped OpenClaw config paths inside the copied env root.",
                "Clone keeps durable agent auth/settings for the same user, but clears copied runtime residue like sessions, logs, and backups.",
                "Background services are not copied; use `start` or `service install` for the clone.",
            ],
        ),
        "export" => render_leaf(
            "Export an environment",
            "Write a portable environment archive that can be imported later.",
            vec![format!(
                "{cmd} env export <name> [--output <path>] [--raw] [--json]"
            )],
            &[
                ("--output <path>", "Write the archive to a specific path"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the export summary as JSON"),
            ],
            vec![format!(
                "{cmd} env export mira --output ./backups/mira.ocm-env.tar"
            )],
            &[],
        ),
        "import" => render_leaf(
            "Import an environment",
            "Create a new environment from a portable environment archive.",
            vec![format!(
                "{cmd} env import <archive> [--name <name>] [--root <path>] [--raw] [--json]"
            )],
            &[
                ("--name <name>", "Override the imported environment name"),
                ("--root <path>", "Override the imported environment root"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the imported environment summary as JSON"),
            ],
            vec![format!(
                "{cmd} env import ./backups/mira.ocm-env.tar --name rowan"
            )],
            &[
                "Imported environments get a fresh identity in the central env registry.",
                "Import rewrites env-scoped OpenClaw config paths for the new root.",
                "Import keeps durable agent auth/settings, but clears copied runtime residue like sessions, logs, and backups.",
            ],
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
                format!("{cmd} env show mira"),
                format!("{cmd} env show mira --raw"),
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
                format!("{cmd} env status mira"),
                format!("{cmd} env status mira --raw"),
                format!("{cmd} env status mira --json"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "doctor" => render_leaf(
            "Inspect environment health",
            "Report environment problems, binding drift, and env-scoped OpenClaw config issues without changing anything.",
            vec![format!("{cmd} env doctor <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print doctor findings as JSON"),
            ],
            vec![
                format!("{cmd} env doctor mira"),
                format!("{cmd} env doctor mira --raw"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "cleanup" => render_leaf(
            "Repair safe environment issues",
            "Preview or apply narrow, safe repairs such as missing binding cleanup and env-scoped OpenClaw config rewrites.",
            vec![format!(
                "{cmd} env cleanup (<name> | --all) [--yes] [--raw] [--json]"
            )],
            &[
                (
                    "--all",
                    "Operate on every environment with actionable repairs",
                ),
                ("--yes", "Apply repairs instead of showing a preview"),
                (
                    "--raw",
                    "Force plain output instead of the TTY receipt view",
                ),
                ("--json", "Print cleanup summaries as JSON"),
            ],
            vec![
                format!("{cmd} env cleanup mira"),
                format!("{cmd} env cleanup mira --yes"),
                format!("{cmd} env cleanup --all --yes"),
            ],
            &["Only a narrow set of safe repairs is applied."],
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
                format!("eval \"$({cmd} env use mira)\""),
                format!("{cmd} env use mira --shell zsh"),
            ],
            &["This command prints shell code. Use `eval` to apply it."],
        ),
        "exec" => render_leaf(
            "Run a command inside an environment",
            "Run any command with the target environment's OpenClaw variables injected.",
            vec![format!("{cmd} env exec <name> -- <command...>")],
            &[],
            vec![
                format!("{cmd} env exec mira -- env | rg OPENCLAW"),
                format!("{cmd} env exec mira -- openclaw status"),
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
                format!("{cmd} env resolve mira"),
                format!("{cmd} env resolve mira --raw"),
                format!("{cmd} env resolve mira --launcher dev -- onboard"),
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
                format!("{cmd} env run mira -- onboard"),
                format!("{cmd} env run mira -- status"),
                format!("{cmd} -- status"),
                format!("{cmd} @mira -- status"),
                format!("{cmd} env run mira --launcher dev -- gateway run"),
            ],
            &[
                "`--` is required before OpenClaw arguments.",
                "If an environment is active, you can also use the root-level `--` shortcut.",
                "For one-shot explicit env runs, use the root-level `@<env>` shortcut.",
            ],
        ),
        "set-runtime" => render_leaf(
            "Bind or clear a runtime",
            "Set the default runtime for an environment, clear it with `none`, or bind an official OpenClaw release directly.",
            vec![
                format!("{cmd} env set-runtime <name> <runtime|none> [--raw] [--json]"),
                format!(
                    "{cmd} env set-runtime <name> (--version <version> | --channel <channel>) [--raw] [--json]"
                ),
            ],
            &[
                (
                    "--version <version>",
                    "Install or reuse one exact published OpenClaw release and bind it",
                ),
                (
                    "--channel <channel>",
                    "Install or reuse the published release currently tagged for one channel",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the updated environment record as JSON"),
            ],
            vec![
                format!("{cmd} env set-runtime mira stable"),
                format!("{cmd} env set-runtime mira --channel stable"),
                format!("{cmd} env set-runtime mira --version 2026.3.24"),
                format!("{cmd} env set-runtime mira none"),
            ],
            &["Use only one of a runtime name, `--version`, or `--channel`."],
        ),
        "set-launcher" => render_leaf(
            "Bind or clear a launcher",
            "Set the default launcher for an environment, or clear it with `none`.",
            vec![format!(
                "{cmd} env set-launcher <name> <launcher|none> [--raw] [--json]"
            )],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the updated environment record as JSON"),
            ],
            vec![
                format!("{cmd} env set-launcher mira stable"),
                format!("{cmd} env set-launcher mira none"),
            ],
            &[],
        ),
        "protect" => render_leaf(
            "Toggle environment protection",
            "Mark an environment as protected or unprotected for destructive commands.",
            vec![format!(
                "{cmd} env protect <name> <on|off> [--raw] [--json]"
            )],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the updated environment record as JSON"),
            ],
            vec![format!("{cmd} env protect mira on")],
            &[],
        ),
        "destroy" => render_leaf(
            "Destroy an environment",
            "Preview or remove an environment, its env snapshots, and its attached OCM-managed service.",
            vec![format!(
                "{cmd} env destroy <name> [--yes] [--force] [--raw] [--json]"
            )],
            &[
                ("--yes", "Apply destruction instead of showing a preview"),
                (
                    "--force",
                    "Override protection checks and destroy the env anyway",
                ),
                ("--raw", "Force plain output instead of TTY cards"),
                ("--json", "Print the destroy preview or result as JSON"),
            ],
            vec![
                format!("{cmd} env destroy mira"),
                format!("{cmd} env destroy mira --yes"),
                format!("{cmd} env destroy mira --yes --force"),
            ],
            &[
                "Destroy removes env snapshots for that env and uninstalls its OCM-managed service when present.",
                "Destroy does not remove shared runtimes or launchers.",
                "If the separate machine-wide OpenClaw service is using the env, destroy refuses to apply.",
                "TTY output uses cards by default. Piped output stays plain.",
            ],
        ),
        "remove" | "rm" => render_leaf(
            "Remove an environment",
            "Delete an environment root and metadata, subject to safety rails.",
            vec![format!(
                "{cmd} env remove <name> [--force] [--raw] [--json]"
            )],
            &[
                ("--force", "Override protection for the target environment"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the removed environment record as JSON"),
            ],
            vec![
                format!("{cmd} env remove mira"),
                format!("{cmd} env remove mira --force"),
            ],
            &["Protected environments require `--force`."],
        ),
        "prune" => render_leaf(
            "Prune old environments",
            "Preview or remove unused environments older than a threshold.",
            vec![format!(
                "{cmd} env prune [--older-than <days>] [--yes] [--raw] [--json]"
            )],
            &[
                (
                    "--older-than <days>",
                    "Age threshold in days. Defaults to 14",
                ),
                ("--yes", "Apply removals instead of showing a preview"),
                (
                    "--raw",
                    "Force plain output instead of the TTY receipt view",
                ),
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

pub fn release_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "install" => render_leaf(
            "Install a published OpenClaw release",
            "Install a published OpenClaw release as a local managed runtime.",
            vec![format!(
                "{cmd} release install [<name>] (--version <version> | --channel <channel>) [--description <text>] [--force] [--raw] [--json]"
            )],
            &[
                (
                    "--version <version>",
                    "Install one exact published OpenClaw version",
                ),
                (
                    "--channel <channel>",
                    "Install the published release currently tagged for one channel",
                ),
                ("--description <text>", "Optional human description"),
                (
                    "--force",
                    "Replace an existing managed runtime of the same name",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the installed runtime record as JSON"),
            ],
            vec![
                format!("{cmd} release install --channel stable"),
                format!("{cmd} release install --channel beta"),
                format!("{cmd} release install --version 2026.3.24"),
            ],
            &[
                "Official installs use canonical runtime names derived from the selector.",
                "Official release installs prefer host Node.js >= 22.14.0 and npm.",
                "On supported platforms, OCM can manage a private copy when they are missing.",
                "Use `ocm doctor host` only if you want a full machine check or an explicit host-tool fix like git.",
            ],
        ),
        "list" => render_leaf(
            "List published OpenClaw releases",
            "Show the published OpenClaw releases available from the official release source.",
            vec![format!(
                "{cmd} release list [--version <version> | --channel <channel>] [--raw] [--json]"
            )],
            &[
                (
                    "--version <version>",
                    "Filter to one exact published version",
                ),
                (
                    "--channel <channel>",
                    "Filter to the release currently tagged for one channel",
                ),
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print releases as JSON"),
            ],
            vec![
                format!("{cmd} release list"),
                format!("{cmd} release list --channel stable"),
                format!("{cmd} release list --version 2026.3.24"),
            ],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "show" => render_leaf(
            "Show a published OpenClaw release",
            "Print metadata for one published OpenClaw release selected by version or channel.",
            vec![format!(
                "{cmd} release show (<version> | --version <version> | --channel <channel>) [--raw] [--json]"
            )],
            &[
                ("--version <version>", "Show one exact published version"),
                (
                    "--channel <channel>",
                    "Show the published release currently tagged for one channel",
                ),
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the release metadata as JSON"),
            ],
            vec![
                format!("{cmd} release show 2026.3.24"),
                format!("{cmd} release show --version 2026.3.24"),
                format!("{cmd} release show --channel stable"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        _ => return None,
    })
}

pub fn env_snapshot_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "create" => render_leaf(
            "Create an environment snapshot",
            "Capture a point-in-time snapshot of an environment.",
            vec![format!(
                "{cmd} env snapshot create <name> [--label <label>] [--raw] [--json]"
            )],
            &[
                ("--label <label>", "Add a human label to the snapshot"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the snapshot summary as JSON"),
            ],
            vec![format!(
                "{cmd} env snapshot create mira --label before-upgrade"
            )],
            &[],
        ),
        "show" => render_leaf(
            "Show one environment snapshot",
            "Print metadata for a single snapshot.",
            vec![format!(
                "{cmd} env snapshot show <name> <snapshot> [--raw] [--json]"
            )],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the snapshot summary as JSON"),
            ],
            vec![
                format!("{cmd} env snapshot show mira 1742922000-123456789"),
                format!("{cmd} env snapshot show mira 1742922000-123456789 --raw"),
            ],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "list" => render_leaf(
            "List environment snapshots",
            "List snapshots for one environment or for all environments.",
            vec![
                format!("{cmd} env snapshot list <name> [--raw] [--json]"),
                format!("{cmd} env snapshot list --all [--raw] [--json]"),
            ],
            &[
                (
                    "--raw",
                    "Force plain line output instead of TTY table rendering",
                ),
                ("--json", "Print snapshot summaries as JSON"),
            ],
            vec![
                format!("{cmd} env snapshot list mira"),
                format!("{cmd} env snapshot list mira --raw"),
                format!("{cmd} env snapshot list --all --json"),
            ],
            &["TTY output renders a table by default. Piped output stays plain."],
        ),
        "restore" => render_leaf(
            "Restore an environment snapshot",
            "Replace an environment root with the contents of a snapshot.",
            vec![format!(
                "{cmd} env snapshot restore <name> <snapshot> [--raw] [--json]"
            )],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the restore summary as JSON"),
            ],
            vec![format!(
                "{cmd} env snapshot restore mira 1742922000-123456789"
            )],
            &["Snapshot restore keeps existing safety rails around foreign directories."],
        ),
        "remove" => render_leaf(
            "Remove an environment snapshot",
            "Delete snapshot metadata and archived content for a snapshot.",
            vec![format!(
                "{cmd} env snapshot remove <name> <snapshot> [--raw] [--json]"
            )],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the removal summary as JSON"),
            ],
            vec![format!(
                "{cmd} env snapshot remove mira 1742922000-123456789"
            )],
            &[],
        ),
        "prune" => render_leaf(
            "Prune environment snapshots",
            "Preview or remove older snapshots for one environment or all environments.",
            vec![format!(
                "{cmd} env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--raw] [--yes] [--json]"
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
                (
                    "--raw",
                    "Force plain preview and result output instead of TTY table rendering",
                ),
                ("--yes", "Apply removals instead of showing a preview"),
                ("--json", "Print prune summaries as JSON"),
            ],
            vec![
                format!("{cmd} env snapshot prune mira --keep 5"),
                format!("{cmd} env snapshot prune mira --keep 5 --yes"),
                format!("{cmd} env snapshot prune --all --older-than 30 --json"),
            ],
            &["TTY output renders tables for preview and applied removals by default."],
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
                "{cmd} launcher add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>] [--raw] [--json]"
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
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
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
            vec![format!("{cmd} launcher show <name> [--raw] [--json]")],
            &[
                ("--raw", "Force plain key/value output instead of TTY cards"),
                ("--json", "Print the launcher as JSON"),
            ],
            vec![format!("{cmd} launcher show stable")],
            &[],
        ),
        "remove" | "rm" => render_leaf(
            "Remove a launcher",
            "Delete a launcher definition.",
            vec![format!("{cmd} launcher remove <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the removed launcher record as JSON"),
            ],
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
                "{cmd} runtime add <name> --path <binary> [--description <text>] [--raw] [--json]"
            )],
            &[
                ("--path <binary>", "Filesystem path to the OpenClaw binary"),
                ("--description <text>", "Optional human description"),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the runtime record as JSON"),
            ],
            vec![format!("{cmd} runtime add stable --path /path/to/openclaw")],
            &[],
        ),
        "install" => render_leaf(
            "Install a managed runtime",
            "Install a runtime from the official OpenClaw release source, a local binary, a direct URL, or a custom release manifest.",
            vec![format!(
                "{cmd} runtime install [<name>] (--version <version> | --channel <channel> | --path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force] [--raw] [--json]"
            )],
            &[
                (
                    "--version <version>",
                    "Install an exact published OpenClaw release",
                ),
                (
                    "--channel <channel>",
                    "Install the published release currently tagged for one channel",
                ),
                ("--path <binary>", "Install from a local binary path"),
                ("--url <url>", "Install from a direct binary URL"),
                (
                    "--manifest-url <url>",
                    "Use a release manifest as the install source",
                ),
                ("--description <text>", "Optional human description"),
                (
                    "--force",
                    "Replace an existing managed runtime of the same name",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the runtime record as JSON"),
            ],
            vec![
                format!("{cmd} runtime install --channel stable"),
                format!("{cmd} runtime install managed-stable --path ./target/debug/openclaw"),
                format!(
                    "{cmd} runtime install nightly --url https://example.test/openclaw-nightly"
                ),
                format!(
                    "{cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable"
                ),
            ],
            &[
                "Exactly one install source must be provided.",
                "Official installs use canonical runtime names unless you reuse the same canonical name explicitly.",
                "Official release installs prefer host Node.js >= 22.14.0 and npm.",
                "On supported platforms, OCM can manage a private copy when they are missing.",
                "Use `ocm doctor host` only if you want a full machine check or an explicit host-tool fix like git.",
            ],
        ),
        "update" => render_leaf(
            "Update managed runtimes",
            "Update one runtime or every managed runtime using stored release provenance.",
            vec![format!(
                "{cmd} runtime update (<name> | --all) [--version <version> | --channel <channel>] [--raw] [--json]"
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
                (
                    "--raw",
                    "Force plain output instead of TTY receipts or tables",
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
            "Inspect OpenClaw releases",
            "Show releases from the official OpenClaw source or from a custom manifest without installing them.",
            vec![format!(
                "{cmd} runtime releases [--manifest-url <url>] [--version <version> | --channel <channel>] [--json]"
            )],
            &[
                (
                    "--manifest-url <url>",
                    "Use a custom manifest instead of the official OpenClaw source",
                ),
                (
                    "--version <version>",
                    "Select one release by explicit version",
                ),
                ("--channel <channel>", "Select one release by channel"),
                ("--json", "Print releases as JSON"),
            ],
            vec![
                format!("{cmd} runtime releases --channel stable"),
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
            vec![format!("{cmd} runtime show <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain key/value output instead of TTY card rendering",
                ),
                ("--json", "Print the runtime record as JSON"),
            ],
            vec![format!("{cmd} runtime show stable")],
            &["TTY output uses grouped cards by default. Piped output stays plain."],
        ),
        "verify" => render_leaf(
            "Verify runtimes",
            "Check runtime health for one runtime or every runtime.",
            vec![format!(
                "{cmd} runtime verify (<name> | --all) [--raw] [--json]"
            )],
            &[
                ("--all", "Verify every runtime"),
                (
                    "--raw",
                    "Force plain verification output instead of TTY cards or tables",
                ),
                ("--json", "Print verification summaries as JSON"),
            ],
            vec![
                format!("{cmd} runtime verify stable"),
                format!("{cmd} runtime verify --all"),
            ],
            &["TTY output uses cards for one runtime and a table for `--all` by default."],
        ),
        "which" => render_leaf(
            "Print a runtime binary path",
            "Show the resolved binary path for a runtime.",
            vec![format!("{cmd} runtime which <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain path output instead of the TTY card view",
                ),
                ("--json", "Print the resolution summary as JSON"),
            ],
            vec![format!("{cmd} runtime which stable")],
            &["TTY output uses a grouped card by default."],
        ),
        "remove" | "rm" => render_leaf(
            "Remove a runtime",
            "Delete a runtime record.",
            vec![format!("{cmd} runtime remove <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the removed runtime record as JSON"),
            ],
            vec![format!("{cmd} runtime remove stable")],
            &[],
        ),
        _ => return None,
    })
}

pub fn service_command_help(cmd: &str, action: &str) -> Option<String> {
    Some(match action {
        "install" => render_leaf(
            "Enable an env in the OCM background service",
            "Enable one env in the shared OCM background service without marking it as running yet.",
            vec![format!("{cmd} service install <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the install summary as JSON"),
            ],
            vec![format!("{cmd} service install mira")],
            &[
                "Use `service start` to start the env after it is enabled.",
                "The shared OCM background service is installed automatically when needed.",
            ],
        ),
        "status" => render_leaf(
            "Show supervised env status",
            "Inspect one env or every env managed by the OCM background service.",
            vec![
                format!("{cmd} service status [env] [--raw] [--json]"),
                format!("{cmd} service status --all [--raw] [--json]"),
            ],
            &[
                ("--raw", "Force plain output instead of TTY cards or tables"),
                ("--all", "Show every environment service"),
                ("--json", "Print service summaries as JSON"),
            ],
            vec![
                format!("{cmd} service status mira"),
                format!("{cmd} service status"),
                format!("{cmd} service status mira --raw"),
                format!("{cmd} service status --all"),
            ],
            &[
                "TTY output uses cards for one env and a table when no env is passed or `--all` is used.",
            ],
        ),
        "start" => render_leaf(
            "Start an env under the background service",
            "Mark one env as running and ensure the shared OCM background service is running.",
            vec![format!("{cmd} service start <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service start mira")],
            &[],
        ),
        "stop" => render_leaf(
            "Stop an env under the background service",
            "Mark one env as stopped without disabling it.",
            vec![format!("{cmd} service stop <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service stop mira")],
            &[],
        ),
        "restart" => render_leaf(
            "Restart an env under the background service",
            "Restart one env under the shared OCM background service.",
            vec![format!("{cmd} service restart <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service restart mira")],
            &[],
        ),
        "uninstall" => render_leaf(
            "Disable an env in the background service",
            "Disable one env in the shared OCM background service.",
            vec![format!("{cmd} service uninstall <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service uninstall mira")],
            &["This does not remove the shared OCM background service."],
        ),
        _ => return None,
    })
}
