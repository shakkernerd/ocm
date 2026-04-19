mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

fn extract_port(output: &str, label: &str) -> u32 {
    let prefix = format!("{label}: ");
    let line = output
        .lines()
        .find(|line| line.trim_start().starts_with(&prefix))
        .expect("port line must be present")
        .trim();
    let value = line
        .strip_prefix(&prefix)
        .expect("port line must start with expected prefix")
        .split_whitespace()
        .next()
        .expect("port line must include a numeric value");
    value.parse::<u32>().expect("port value must be numeric")
}

#[test]
fn env_create_prints_the_effective_gateway_port_for_fresh_envs() {
    let root = TestDir::new("env-create-output-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let output = stdout(&created);
    assert!(output.contains("Created env demo"));
    assert!(output.contains("(computed)"));
    assert!(extract_port(&output, "effective gateway port") >= 18_789);
    assert!(output.contains("onboard: ocm @demo -- onboard"));
    assert!(output.contains("run: ocm @demo -- status"));
}

#[test]
fn env_clone_prints_the_effective_gateway_port_for_the_cloned_env() {
    let root = TestDir::new("env-clone-output-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let created_output = stdout(&created);
    let source_port = extract_port(&created_output, "effective gateway port");

    let cloned = run_ocm(&cwd, &env, &["env", "clone", "demo", "copy"]);
    assert!(cloned.status.success(), "{}", stderr(&cloned));
    let output = stdout(&cloned);
    assert!(output.contains("Cloned env copy from demo"));
    let cloned_port = extract_port(&output, "gateway port");
    assert_ne!(cloned_port, source_port);
    assert!(output.contains("service: not copied from source"));
    assert!(output.contains("start: ocm start copy"));
    assert!(output.contains("onboard: ocm @copy -- onboard"));
    assert!(output.contains("run: ocm @copy -- status"));
}
