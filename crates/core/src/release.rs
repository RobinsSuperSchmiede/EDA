//! Releases: a `downloads.json` manifest and a download page that offer every platform's file
//! with one link, and the update check of installed apps (`package.update_url`).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MANIFEST_FILE: &str = "downloads.json";
pub const PAGE_FILE: &str = "index.html";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Downloads {
    pub app: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub downloads: Vec<Download>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Download {
    /// `windows`, `macos`, `linux`, `android` or `any` (app bundle).
    pub platform: String,
    /// `x86_64`, `aarch64`, `universal`, or absent when the file runs everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// `nsis`, `msi`, `dmg`, `appimage`, `deb`, `rpm`, `apk`, `bundle`.
    pub kind: String,
    pub file: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// Platform, kind and architecture of a release file, from its name.
pub fn classify(file: &str) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    let f = file.to_lowercase();
    let (platform, kind) = if f.ends_with(".msi") {
        ("windows", "msi")
    } else if f.ends_with(".exe") {
        ("windows", "nsis")
    } else if f.ends_with(".dmg") {
        ("macos", "dmg")
    } else if f.ends_with(".appimage") {
        ("linux", "appimage")
    } else if f.ends_with(".deb") {
        ("linux", "deb")
    } else if f.ends_with(".rpm") {
        ("linux", "rpm")
    } else if f.ends_with(".apk") {
        ("android", "apk")
    } else if f.ends_with(".osapp") {
        ("any", "bundle")
    } else {
        return None;
    };
    let arch = if f.contains("universal") {
        Some("universal")
    } else if f.contains("aarch64") || f.contains("arm64") {
        Some("aarch64")
    } else if f.contains("x86_64") || f.contains("x64") || f.contains("amd64") {
        Some("x86_64")
    } else {
        None
    };
    Some((platform, kind, arch))
}

/// Build the manifest for all release files in `dir`. `base_url` is where the files will be
/// downloaded from (a GitHub release, a web server); empty = relative links.
pub fn manifest(dir: &Path, app: &str, version: &str, base_url: &str) -> Result<Downloads> {
    let mut downloads = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    for name in names {
        let Some((platform, kind, arch)) = classify(&name) else { continue };
        let path = dir.join(&name);
        let bytes = std::fs::read(&path)?;
        let base = base_url.trim_end_matches('/');
        downloads.push(Download {
            platform: platform.into(),
            arch: arch.map(str::to_string),
            kind: kind.into(),
            url: if base.is_empty() { encode_path(&name) } else { format!("{base}/{}", encode_path(&name)) },
            file: name,
            size: bytes.len() as u64,
            sha256: crate::remote::sha256_hex(&bytes),
        });
    }
    if downloads.is_empty() {
        bail!("no release files (installers, .osapp bundles, .apk) in {}", dir.display());
    }
    Ok(Downloads { app: app.into(), version: version.into(), notes: None, downloads })
}

fn encode_path(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Write `downloads.json` and a download page (`index.html`) into `dir`.
pub fn write(dir: &Path, d: &Downloads) -> Result<()> {
    std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec_pretty(d)?)?;
    std::fs::write(dir.join(PAGE_FILE), page(d))?;
    Ok(())
}

/// A static download page: the button for the visitor's system first, all files below.
pub fn page(d: &Downloads) -> String {
    let label = |x: &Download| -> String {
        let os = match x.platform.as_str() {
            "windows" => "Windows",
            "macos" => "macOS",
            "linux" => "Linux",
            "android" => "Android",
            _ => "Any system (app bundle)",
        };
        let kind = match x.kind.as_str() {
            "nsis" => "installer",
            "msi" => "MSI",
            "dmg" => "disk image",
            "appimage" => "AppImage",
            "deb" => "Debian/Ubuntu",
            "rpm" => "Fedora/openSUSE",
            "apk" => "APK",
            _ => ".osapp – OpenShell app or Termux",
        };
        match &x.arch {
            Some(a) => format!("{os} · {kind} · {a}"),
            None => format!("{os} · {kind}"),
        }
    };
    let rows: String = d
        .downloads
        .iter()
        .map(|x| {
            format!(
                "<li data-platform=\"{}\" data-arch=\"{}\"><a href=\"{}\">{}</a><small>{} · {:.1} MB · sha256 {}</small></li>",
                html_escape(&x.platform),
                html_escape(x.arch.as_deref().unwrap_or("")),
                html_escape(&x.url),
                html_escape(&label(x)),
                html_escape(&x.file),
                x.size as f64 / 1_048_576.0,
                &x.sha256[..16.min(x.sha256.len())],
            )
        })
        .collect();
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Download {app} {version}</title>
<style>
body{{font-family:system-ui,sans-serif;max-width:40rem;margin:3rem auto;padding:0 1rem;color:#1b1b1b;background:#fafafa}}
h1{{font-size:1.6rem;margin-bottom:.2rem}}p.v{{color:#666;margin-top:0}}
a.primary{{display:inline-block;margin:1.2rem 0;padding:.8rem 1.4rem;background:#1b1b1b;color:#fff;border-radius:.5rem;text-decoration:none;font-weight:600}}
ul{{list-style:none;padding:0}}li{{padding:.6rem 0;border-top:1px solid #e4e4e4}}li small{{display:block;color:#777;font-size:.8rem}}
li.match a{{font-weight:700}}
@media (prefers-color-scheme:dark){{body{{background:#121212;color:#eee}}a{{color:#9cf}}a.primary{{background:#eee;color:#121212}}li{{border-color:#333}}}}
</style></head>
<body>
<h1>{app}</h1><p class="v">Version {version}</p>
<a class="primary" id="primary" href="#all">Choose your system below</a>
<h2 id="all">All downloads</h2>
<ul id="files">{rows}</ul>
<p><small>Built with OpenShell. Verify downloads with the SHA-256 checksums in <a href="downloads.json">downloads.json</a>.</small></p>
<script>
(function () {{
  var ua = navigator.userAgent.toLowerCase();
  var os = /android/.test(ua) ? "android" : /windows/.test(ua) ? "windows" : /mac os|macintosh/.test(ua) ? "macos" : /linux|x11/.test(ua) ? "linux" : "";
  var arm = /arm|aarch64/.test(ua);
  var items = Array.prototype.slice.call(document.querySelectorAll("#files li"));
  var score = function (li) {{
    var p = li.dataset.platform, a = li.dataset.arch;
    if (p === os) return 10 + (a === "universal" || !a ? 1 : (arm ? (a === "aarch64" ? 2 : 0) : (a === "x86_64" ? 2 : 0)));
    if (os === "android" && p === "any") return 5;
    return p === "any" ? 1 : 0;
  }};
  var best = items.reduce(function (b, li) {{ return score(li) > (b ? score(b) : 0) ? li : b; }}, null);
  if (best && score(best) >= 5) {{
    var link = best.querySelector("a");
    var primary = document.getElementById("primary");
    primary.href = link.href;
    primary.textContent = "Download for " + link.textContent;
    best.className = "match";
  }}
}})();
</script>
</body></html>
"##,
        app = html_escape(&d.app),
        version = html_escape(&d.version),
        rows = rows
    )
}

/// `1.2.10` > `1.2.9`; pre-release suffixes count as older than the release.
pub fn newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once(['-', '+']) {
            Some((c, _)) => (c, v.contains('-')),
            None => (v, false),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    }
    let (a, a_pre) = parts(candidate);
    let (b, b_pre) = parts(current);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b_pre && !a_pre,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    /// The best file for this system, if the release has one.
    pub download: Option<Download>,
    /// The release's download page.
    pub page: String,
}

/// The download that fits `platform` (`windows`, `macos`, `linux`, `android`) and `arch`.
pub fn best_for<'a>(d: &'a Downloads, platform: &str, arch: &str) -> Option<&'a Download> {
    let score = |x: &Download| -> u32 {
        let arch_ok = match x.arch.as_deref() {
            None | Some("universal") => 1,
            Some(a) if a == arch => 2,
            _ => 0,
        };
        if x.platform == platform && arch_ok > 0 {
            10 + arch_ok
        } else if x.platform == "any" {
            if platform == "android" {
                8
            } else {
                1
            }
        } else {
            0
        }
    };
    d.downloads.iter().filter(|x| score(x) > 0).max_by_key(|x| score(x))
}

/// Fetch the release manifest at `url` and compare it with `current`.
pub async fn check_update(url: &str, current: &str, platform: &str, arch: &str) -> Result<UpdateInfo> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build()?;
    let d: Downloads = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot reach {url}"))?
        .error_for_status()?
        .json()
        .await
        .context("the update URL does not serve a downloads.json")?;
    let page = match url.rsplit_once('/') {
        Some((base, _)) => format!("{base}/{PAGE_FILE}"),
        None => url.to_string(),
    };
    Ok(UpdateInfo {
        current: current.into(),
        available: newer(&d.version, current),
        download: best_for(&d, platform, arch).cloned(),
        latest: d.version,
        page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_release_files() {
        assert_eq!(classify("My App_1.2.0_x64-setup.exe"), Some(("windows", "nsis", Some("x86_64"))));
        assert_eq!(classify("My App_1.2.0_x64_en-US.msi"), Some(("windows", "msi", Some("x86_64"))));
        assert_eq!(classify("My App_1.2.0_aarch64.dmg"), Some(("macos", "dmg", Some("aarch64"))));
        assert_eq!(classify("my-app_1.2.0_amd64.AppImage"), Some(("linux", "appimage", Some("x86_64"))));
        assert_eq!(classify("my-app_1.2.0_amd64.deb"), Some(("linux", "deb", Some("x86_64"))));
        assert_eq!(classify("my-app-1.2.0.osapp"), Some(("any", "bundle", None)));
        assert_eq!(classify("SHA256SUMS.txt"), None);
    }

    #[test]
    fn manifest_page_and_best_download() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "App_1.0.0_x64-setup.exe",
            "App_1.0.0_aarch64.dmg",
            "app_1.0.0_amd64.AppImage",
            "app-1.0.0.osapp",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(f), f).unwrap();
        }
        let d = manifest(dir.path(), "App <1>", "1.0.0", "https://example.com/releases/v1.0.0/").unwrap();
        assert_eq!(d.downloads.len(), 4);
        let exe = d.downloads.iter().find(|x| x.platform == "windows").unwrap();
        assert_eq!(exe.url, "https://example.com/releases/v1.0.0/App_1.0.0_x64-setup.exe");
        assert_eq!(exe.sha256.len(), 64);
        write(dir.path(), &d).unwrap();
        let html = std::fs::read_to_string(dir.path().join(PAGE_FILE)).unwrap();
        assert!(html.contains("App &lt;1&gt;"), "escaped");
        assert!(html.contains("data-platform=\"macos\""));
        let back: Downloads =
            serde_json::from_slice(&std::fs::read(dir.path().join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(back, d);

        assert_eq!(best_for(&d, "windows", "x86_64").unwrap().kind, "nsis");
        assert_eq!(best_for(&d, "macos", "aarch64").unwrap().kind, "dmg");
        assert_eq!(best_for(&d, "macos", "x86_64").unwrap().kind, "bundle", "no Intel build: the bundle");
        assert_eq!(best_for(&d, "android", "aarch64").unwrap().kind, "bundle");
    }

    #[test]
    fn version_comparison() {
        assert!(newer("1.2.10", "1.2.9"));
        assert!(newer("v2.0.0", "1.9.9"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0", "1.0.1"));
        assert!(newer("1.0.0", "1.0.0-beta.1"));
        assert!(!newer("1.0.0-beta.2", "1.0.0"));
    }
}
