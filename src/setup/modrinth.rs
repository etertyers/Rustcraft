//! Modrinth's side: the API (v2) says where each pinned version's file is;
//! the file itself comes from the CDN and is checked against the pinned
//! sha512, so a changed or swapped file is never installed.

use super::download::Http;
use super::manifest::{Endpoints, Extra};
use super::{Result, SetupError};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Version {
    id: String,
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    url: String,
    hashes: Hashes,
}

#[derive(Deserialize)]
struct Hashes {
    #[serde(default)]
    sha512: String,
}

/// Percent-encodes everything but unreserved characters.
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The CDN address Modrinth uses for a file, for when the API can't be asked.
pub fn cdn_url(ep: &Endpoints, extra: &Extra) -> String {
    format!(
        "{}/data/{}/versions/{}/{}",
        ep.modrinth_cdn,
        extra.project,
        extra.version,
        encode(&extra.file)
    )
}

/// The download address of each of `extras`, by file name: one
/// `GET /v2/versions?ids=[…]`, matching each version's file by sha512.
/// If Modrinth answers but doesn't list a pinned file, that is an error. If
/// the API itself fails other than by being unreachable, the CDN's address
/// is used (the checksum still guards the result).
pub fn resolve_urls(
    http: &Http,
    ep: &Endpoints,
    extras: &[&Extra],
) -> Result<HashMap<String, String>> {
    if extras.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = extras
        .iter()
        .map(|e| format!("\"{}\"", e.version))
        .collect::<Vec<_>>()
        .join(",");
    let url = format!(
        "{}/v2/versions?ids={}",
        ep.modrinth_api,
        encode(&format!("[{ids}]"))
    );
    let versions: Vec<Version> = match http.bytes(&url, None, 16 << 20) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| {
            SetupError::Format(format!("Modrinth's answer isn't what was expected: {err}"))
        })?,
        Err(err @ (SetupError::Offline { .. } | SetupError::Cancelled)) => return Err(err),
        Err(err) => {
            log::warn!("Modrinth's API failed ({err}); using its CDN addresses");
            return Ok(extras
                .iter()
                .map(|e| (e.file.clone(), cdn_url(ep, e)))
                .collect());
        }
    };
    let mut urls = HashMap::new();
    for extra in extras {
        let file = versions
            .iter()
            .find(|v| v.id == extra.version)
            .and_then(|v| {
                v.files
                    .iter()
                    .find(|f| f.hashes.sha512.eq_ignore_ascii_case(&extra.sha512))
            })
            .ok_or_else(|| SetupError::ExtraGone(extra.name.clone()))?;
        urls.insert(extra.file.clone(), file.url.clone());
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_plus_spaces_and_brackets() {
        assert_eq!(encode("a b+c"), "a%20b%2Bc");
        assert_eq!(encode("[\"x\"]"), "%5B%22x%22%5D");
    }

    #[test]
    fn the_cdn_address_follows_modrinths_layout() {
        let extra = &super::super::manifest::extras()[0];
        let url = cdn_url(&Endpoints::official(), extra);
        assert!(
            url.starts_with(
                "https://cdn.modrinth.com/data/rox3U8B6/versions/jQBWn2Q3/Bare%20Bones"
            )
        );
    }
}
