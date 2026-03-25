use std::fs::{self, File};
use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;

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
    Ok(Box::new(response.into_reader()))
}
