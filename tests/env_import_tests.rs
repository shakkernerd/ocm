mod support;

use std::{fs, path::Path};

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

#[test]
fn env_import_restores_an_archive_with_a_new_name_and_root() {
    let root = TestDir::new("env-import");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "source", "--port", "19789", "--protect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "hello import",
    );

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-backup.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-backup.tar",
            "--name",
            "target",
            "--root",
            "./imports/target-root",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    let output = stdout(&import);
    assert!(output.contains("Imported env target from source"));

    let show = run_ocm(&cwd, &env, &["env", "show", "target", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_output = stdout(&show);
    assert!(show_output.contains("\"name\": \"target\""));
    assert!(show_output.contains("\"gatewayPort\": 19789"));
    assert!(show_output.contains("\"protected\": true"));

    assert_eq!(
        fs::read_to_string(
            root.child("workspace/imports/target-root/.openclaw/workspace/notes.txt")
        )
        .unwrap(),
        "hello import"
    );
}

#[test]
fn env_import_json_reports_the_archive_and_source_name() {
    let root = TestDir::new("env-import-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let export = run_ocm(&cwd, &env, &["env", "export", "source"]);
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./source.ocm-env.tar",
            "--name",
            "target",
            "--json",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    let output = stdout(&import);
    assert!(output.contains("\"name\": \"target\""));
    assert!(output.contains("\"sourceName\": \"source\""));
    assert!(output.contains("\"archivePath\":"));
}

#[test]
fn env_import_rewrites_openclaw_config_for_the_new_root() {
    let root = TestDir::new("env-import-config-rewrite");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source", "--port", "19789"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let source_root = root.child("ocm-home/envs/source");
    fs::write(
        source_root.join(".openclaw/openclaw.json"),
        format!(
            "{{\n  \"agents\": {{\n    \"defaults\": {{\n      \"workspace\": \"{}\"\n    }}\n  }},\n  \"gateway\": {{\n    \"port\": 19789\n  }}\n}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    )
    .unwrap();

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-config.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-config.tar",
            "--name",
            "target",
            "--root",
            "./imports/target-root",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));

    let raw =
        fs::read_to_string(root.child("workspace/imports/target-root/.openclaw/openclaw.json"))
            .unwrap();
    let config: Value = serde_json::from_str(&raw).unwrap();
    let actual_workspace = fs::canonicalize(Path::new(
        config["agents"]["defaults"]["workspace"].as_str().unwrap(),
    ))
    .unwrap();
    let expected_workspace = fs::canonicalize(root.child("workspace/imports/target-root"))
        .unwrap()
        .join(".openclaw/workspace");
    assert_eq!(actual_workspace, expected_workspace);
    assert_eq!(config["gateway"]["port"].as_u64(), Some(19789));
}
