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
        "Manage isolated OpenClaw environments, releases, runtimes, launchers, and services."
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
            ("release", "Published OpenClaw releases and release details"),
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
            format!("{cmd} start mybot"),
            format!("{cmd} @mybot -- onboard"),
            format!("{cmd} @mybot -- status"),
            format!("{cmd} upgrade mybot"),
            format!("{cmd} start mybot --channel beta"),
            format!(
                "{cmd} start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service"
            ),
        ]),
    );
    push_section(
        &mut lines,
        "More",
        format_examples(&[
            format!("{cmd} help setup"),
            format!("{cmd} help start"),
            format!("{cmd} help upgrade"),
            format!("{cmd} help doctor"),
            format!("{cmd} doctor host"),
            format!("{cmd} help self"),
            format!("{cmd} help env"),
            format!("{cmd} help release"),
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
            "Run `ocm doctor host` if you want a full machine check before choosing a release flow.",
            "When run inside an OpenClaw checkout, local mode defaults to `pnpm openclaw` in that folder.",
            "Use `start` when you already know the source you want.",
        ],
    )
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
            format!("{cmd} start demo --channel stable"),
            format!("{cmd} start demo --version 2026.3.24"),
            format!(
                "{cmd} start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw --no-onboard"
            ),
            format!(
                "{cmd} start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service --no-onboard"
            ),
        ],
        &[
            "If an environment already exists, start reuses it and only adjusts binding/protection when you asked for it.",
            "Start installs and starts the env service by default. Use `--no-service` when you do not want a background process.",
            "Official release selectors prefer host Node.js >= 22.14.0 and npm, and OCM can manage a private copy on supported platforms when they are missing.",
            "Run `ocm doctor host` to check the host before using official release selectors.",
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
            format!("{cmd} upgrade mybot"),
            format!("{cmd} upgrade mybot --channel beta"),
            format!("{cmd} upgrade mybot --version 2026.3.24"),
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
            vec![format!("{cmd} doctor host [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain host-check output instead of TTY card rendering",
                ),
                ("--json", "Print the host-check summary as JSON"),
            ],
            vec![format!("{cmd} doctor host")],
            &[
                "Official release installs prefer host Node.js >= 22.14.0 and npm.",
                "On supported platforms, OCM can manage a private copy when they are missing.",
                "Recommended tools are advisory; they do not block local-command or launcher flows.",
            ],
        ),
        _ => return None,
    })
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
                format!("{cmd} env create demo --launcher stable"),
                format!("{cmd} env create demo --channel stable"),
                format!("{cmd} env create pinned --version 2026.3.24"),
            ],
            &[
                "Environments are the main isolation unit in OCM.",
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
            vec![format!("{cmd} env clone demo demo-copy")],
            &[
                "Clone resets environment identity while preserving the copied state.",
                "Clone assigns a fresh gateway port to the new env to avoid collisions.",
                "Clone rewrites env-scoped OpenClaw config paths inside the copied env root.",
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
                "{cmd} env export demo --output ./backups/demo.ocm-env.tar"
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
                format!("{cmd} env cleanup demo"),
                format!("{cmd} env cleanup demo --yes"),
                format!("{cmd} env cleanup --all --yes"),
            ],
            &["Only a narrow set of safe repairs is applied."],
        ),
        "repair-marker" => render_leaf(
            "Repair an environment marker",
            "Rewrite `.ocm-env.json` for a known environment root.",
            vec![format!("{cmd} env repair-marker <name> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain output instead of the TTY receipt view",
                ),
                ("--json", "Print the repair summary as JSON"),
            ],
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
                format!("{cmd} env set-runtime demo stable"),
                format!("{cmd} env set-runtime demo --channel stable"),
                format!("{cmd} env set-runtime demo --version 2026.3.24"),
                format!("{cmd} env set-runtime demo none"),
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
                format!("{cmd} env set-launcher demo stable"),
                format!("{cmd} env set-launcher demo none"),
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
            vec![format!("{cmd} env protect demo on")],
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
                    "Override protection and missing-marker safety rails",
                ),
                ("--raw", "Force plain output instead of TTY cards"),
                ("--json", "Print the destroy preview or result as JSON"),
            ],
            vec![
                format!("{cmd} env destroy demo"),
                format!("{cmd} env destroy demo --yes"),
                format!("{cmd} env destroy demo --yes --force"),
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
                format!("{cmd} env remove demo"),
                format!("{cmd} env remove demo --force"),
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
                "Run `ocm doctor host` on a new machine before using official release installs.",
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
                "{cmd} env snapshot create demo --label before-upgrade"
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
                format!("{cmd} env snapshot show demo 1742922000-123456789"),
                format!("{cmd} env snapshot show demo 1742922000-123456789 --raw"),
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
                format!("{cmd} env snapshot list demo"),
                format!("{cmd} env snapshot list demo --raw"),
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
                "{cmd} env snapshot restore demo 1742922000-123456789"
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
                "{cmd} env snapshot remove demo 1742922000-123456789"
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
                format!("{cmd} env snapshot prune demo --keep 5"),
                format!("{cmd} env snapshot prune demo --keep 5 --yes"),
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
                "Run `ocm doctor host` on a new machine before using official release installs.",
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
            vec![format!("{cmd} runtime which <name> [--json]")],
            &[("--json", "Print the resolution summary as JSON")],
            vec![format!("{cmd} runtime which stable")],
            &[],
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
            "Move the legacy machine-wide OpenClaw service into the env-scoped OCM service model.",
            vec![format!(
                "{cmd} service adopt-global <env> [--dry-run] [--raw] [--json]"
            )],
            &[
                (
                    "--dry-run",
                    "Preview adoption without mutating files or service-manager state",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
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
                "{cmd} service restore-global <env> [--dry-run] [--raw] [--json]"
            )],
            &[
                (
                    "--dry-run",
                    "Preview the restore without mutating files or service-manager state",
                ),
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the restore summary as JSON"),
            ],
            vec![format!("{cmd} service restore-global demo --dry-run")],
            &[],
        ),
        "install" => render_leaf(
            "Install an env-scoped service",
            "Create a persistent service for an environment using the current binding and effective port.",
            vec![format!("{cmd} service install <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the install summary as JSON"),
            ],
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
            vec![format!("{cmd} service start <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service start demo")],
            &[],
        ),
        "stop" => render_leaf(
            "Stop a service",
            "Stop an installed env-scoped service.",
            vec![format!("{cmd} service stop <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service stop demo")],
            &[],
        ),
        "restart" => render_leaf(
            "Restart a service",
            "Restart an installed env-scoped service.",
            vec![format!("{cmd} service restart <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service restart demo")],
            &[],
        ),
        "uninstall" => render_leaf(
            "Uninstall a service",
            "Remove an env-scoped service definition.",
            vec![format!("{cmd} service uninstall <env> [--raw] [--json]")],
            &[
                (
                    "--raw",
                    "Force plain line output instead of the TTY receipt view",
                ),
                ("--json", "Print the action summary as JSON"),
            ],
            vec![format!("{cmd} service uninstall demo")],
            &[],
        ),
        _ => return None,
    })
}
