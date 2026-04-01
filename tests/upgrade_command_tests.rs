mod support;

use std::fs;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use sha2::{Digest, Sha512};
use tar::{Builder, Header};

use crate::support::{
    TestDir, TestHttpServer, install_fake_launchctl, install_fake_node_and_npm, ocm_env, run_ocm,
    stderr, stdout,
};

fn append_tar_file(
    builder: &mut Builder<&mut GzEncoder<Vec<u8>>>,
    path: &str,
    body: &[u8],
    mode: u32,
) {
    let mut header = Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    builder.append_data(&mut header, path, body).unwrap();
}

fn openclaw_package_tarball(script_body: &str, version: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);
        append_tar_file(
            &mut builder,
            "package/openclaw.mjs",
            script_body.as_bytes(),
            0o755,
        );
        append_tar_file(
            &mut builder,
            "package/package.json",
            format!(
                "{{\"name\":\"openclaw\",\"version\":\"{version}\",\"bin\":{{\"openclaw\":\"openclaw.mjs\"}}}}"
            )
            .as_bytes(),
            0o644,
        );
        builder.finish().unwrap();
    }
    encoder.finish().unwrap()
}

fn sha512_integrity(body: &[u8]) -> String {
    let digest = Sha512::digest(body);
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

#[test]
fn upgrade_updates_a_tracked_runtime_and_restarts_the_service() {
    let root = TestDir::new("upgrade-tracked-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );

    let new_tarball = openclaw_package_tarball("console.log('2026.3.25');\n", "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(&root, &mut env);

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-onboard"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=updated"), "{output}");
    assert!(output.contains("service=restarted"), "{output}");
    assert!(output.contains("version=2026.3.25"), "{output}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.25");
}

#[test]
fn upgrade_reports_pinned_envs_without_moving_them() {
    let root = TestDir::new("upgrade-pinned");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "pinned",
            "--version",
            "2026.3.24",
            "--no-service",
            "--no-onboard",
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "pinned"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=pinned"), "{output}");
    assert!(output.contains("exact release"), "{output}");
}

#[test]
fn upgrade_can_switch_a_local_launcher_env_to_a_published_runtime() {
    let root = TestDir::new("upgrade-launcher-to-runtime");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();

    let tarball = openclaw_package_tarball("console.log('stable');\n", "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "hacking",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.display().to_string(),
            "--no-service",
            "--no-onboard",
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "hacking", "--channel", "stable"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("from=launcher:hacking.local"), "{output}");
    assert!(output.contains("to=runtime:stable"), "{output}");
    assert!(output.contains("outcome=switched"), "{output}");

    let show = run_ocm(&cwd, &env, &["env", "show", "hacking", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "stable");
    assert!(env_json["defaultLauncher"].is_null());
}

#[test]
fn upgrade_all_updates_safe_envs_and_skips_local_or_pinned_ones() {
    let root = TestDir::new("upgrade-all");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();

    let old_tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball = openclaw_package_tarball("console.log('2026.3.25');\n", "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let stable = run_ocm(
        &cwd,
        &env,
        &["start", "stable-env", "--no-service", "--no-onboard"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));
    let pinned = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "pinned-env",
            "--version",
            "2026.3.24",
            "--no-service",
            "--no-onboard",
        ],
    );
    assert!(pinned.status.success(), "{}", stderr(&pinned));
    let local = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "local-env",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.display().to_string(),
            "--no-service",
            "--no-onboard",
        ],
    );
    assert!(local.status.success(), "{}", stderr(&local));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "--all", "--json"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let json: Value = serde_json::from_str(&stdout(&upgrade)).unwrap();
    assert_eq!(json["count"], 3);
    assert_eq!(json["changed"], 1);
    assert_eq!(json["current"], 0);
    assert_eq!(json["skipped"], 2);
    assert_eq!(json["failed"], 0);
}
