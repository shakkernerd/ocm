mod support;

use std::fs;

use ocm::download::{
    artifact_file_name_from_url, download_to_file, file_sha256, normalize_sha256,
    verify_file_sha256,
};

use crate::support::{TestDir, TestHttpServer};

#[test]
fn artifact_file_name_from_url_uses_the_last_path_segment() {
    let file_name =
        artifact_file_name_from_url("https://example.test/releases/openclaw-macos?sig=1#part")
            .unwrap();
    assert_eq!(file_name, "openclaw-macos");
}

#[test]
fn artifact_file_name_from_url_requires_a_file_name() {
    let error = artifact_file_name_from_url("https://example.test/releases/").unwrap_err();
    assert!(error.contains("runtime URL must include a file name"));
}

#[test]
fn download_to_file_fetches_the_http_response_body() {
    let root = TestDir::new("download-helper-success");
    let destination = root.child("downloads/openclaw");
    let server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw",
        "application/octet-stream",
        b"openclaw-binary",
    );

    download_to_file(&server.url(), &destination).unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"openclaw-binary");
}

#[test]
fn download_to_file_reports_http_errors() {
    let root = TestDir::new("download-helper-missing");
    let destination = root.child("downloads/openclaw");
    let server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw",
        "application/octet-stream",
        b"openclaw-binary",
    );

    let error = download_to_file(&format!("{}/missing", server.url()), &destination).unwrap_err();

    assert!(error.contains("failed to download runtime URL"));
    assert!(error.contains("404"));
}

#[test]
fn file_sha256_and_verify_file_sha256_report_the_expected_digest() {
    let root = TestDir::new("download-helper-sha256");
    let artifact = root.child("downloads/openclaw");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"openclaw-binary").unwrap();

    let digest = file_sha256(&artifact).unwrap();
    assert_eq!(
        digest,
        "27267fa2dc8f0e5b3e0d4d606bb5c2608fccfba413d87fa350598d5cb16545d1"
    );
    assert_eq!(
        verify_file_sha256(
            &artifact,
            "27267FA2DC8F0E5B3E0D4D606BB5C2608FCCFBA413D87FA350598D5CB16545D1"
        )
        .unwrap(),
        digest
    );
}

#[test]
fn normalize_sha256_rejects_invalid_hashes() {
    let error = normalize_sha256("1234").unwrap_err();
    assert!(error.contains("sha256 is invalid"));
}

#[test]
fn verify_file_sha256_reports_mismatches() {
    let root = TestDir::new("download-helper-sha256-mismatch");
    let artifact = root.child("downloads/openclaw");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"openclaw-binary").unwrap();

    let error = verify_file_sha256(
        &artifact,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap_err();
    assert!(error.contains("sha256 mismatch"));
    assert!(error.contains("expected aaaaaaaa"));
}
