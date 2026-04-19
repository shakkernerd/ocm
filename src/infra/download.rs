use std::fs::{self, File};
use std::io;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256, Sha512};

pub fn artifact_file_name_from_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("runtime URL is required".to_string());
    }

    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let segment = without_query
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("runtime URL must include a file name: {trimmed}"))?;

    if segment == "." || segment == ".." {
        return Err(format!("runtime URL must include a file name: {trimmed}"));
    }

    Ok(segment.to_string())
}

pub fn download_to_file(url: &str, destination: &Path) -> Result<(), String> {
    let mut reader = open_url_reader(url)?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut file = File::create(destination).map_err(|error| error.to_string())?;
    io::copy(&mut reader, &mut file).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let reader = open_url_reader(url)?;
    serde_json::from_reader(reader)
        .map_err(|error| format!("failed to parse runtime URL \"{}\": {error}", url.trim()))
}

fn open_url_reader(url: &str) -> Result<Box<dyn io::Read>, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("runtime URL is required".to_string());
    }

    let response = ureq::get(trimmed)
        .call()
        .map_err(|error| format!("failed to download runtime URL \"{trimmed}\": {error}"))?;
    Ok(Box::new(response.into_body().into_reader()))
}

pub fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn normalize_sha256(expected: &str) -> Result<String, String> {
    let value = expected.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err("runtime artifact sha256 is required".to_string());
    }
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("runtime artifact sha256 is invalid: {expected}"));
    }
    Ok(value)
}

pub fn verify_file_sha256(path: &Path, expected: &str) -> Result<String, String> {
    let expected = normalize_sha256(expected)?;
    let actual = file_sha256(path)?;
    if actual != expected {
        return Err(format!(
            "runtime artifact sha256 mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(actual)
}

pub fn verify_file_integrity(path: &Path, expected: &str) -> Result<(), String> {
    let expected = expected.trim();
    if expected.is_empty() {
        return Err("runtime artifact integrity is required".to_string());
    }

    let Some((algorithm, encoded)) = expected.split_once('-') else {
        return Err(format!("runtime artifact integrity is invalid: {expected}"));
    };
    if encoded.trim().is_empty() {
        return Err(format!("runtime artifact integrity is invalid: {expected}"));
    }

    match algorithm {
        "sha512" => verify_file_sha512_base64(path, encoded.trim(), expected),
        _ => Err(format!(
            "runtime artifact integrity algorithm is unsupported: {algorithm}"
        )),
    }
}

fn verify_file_sha512_base64(
    path: &Path,
    expected_base64: &str,
    raw_expected: &str,
) -> Result<(), String> {
    let expected = base64::engine::general_purpose::STANDARD
        .decode(expected_base64)
        .map_err(|_| format!("runtime artifact integrity is invalid: {raw_expected}"))?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha512::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual = hasher.finalize();
    if actual.as_slice() != expected.as_slice() {
        return Err("runtime artifact integrity mismatch".to_string());
    }
    Ok(())
}
