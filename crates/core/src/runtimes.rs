//! Download language runtimes into a project (`openshell runtime install`).
//!
//! | Runtime | Source | Archive |
//! |---|---|---|
//! | `node` | nodejs.org (`dist/index.json`, `SHASUMS256.txt`) | zip (Windows), tar.gz |
//! | `python` | python-build-standalone, `install_only` builds (GitHub release, `SHA256SUMS`) | tar.gz |
//! | `java` | Eclipse Temurin JRE (api.adoptium.net, checksum in the API answer) | zip (Windows), tar.gz |
//!
//! Every download is checked against its SHA-256 before it is unpacked. The runtime ends up in
//! `runtimes/<name>/` (or the configured `path`) with a `.openshell-runtime.json` that records
//! version, source and URL. A runtime is built for one operating system and architecture: build
//! installers on each system (the release workflow does) instead of copying a runtime folder.

use anyhow::{anyhow, bail, Context, Result};
use openshell_config::runtimes::{runtime, Version};
use openshell_config::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MARKER: &str = ".openshell-runtime.json";

/// What `.openshell-runtime.json` records about an installed runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub source: String,
    pub url: String,
    pub sha256: String,
}

/// The installed runtime in `dir`, if any.
pub fn installed(dir: &Path) -> Option<Installed> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(MARKER)).ok()?).ok()
}

/// A concrete file to download.
#[derive(Debug, Clone)]
pub struct Download {
    pub version: String,
    pub url: String,
    pub file: String,
    pub sha256: String,
    pub source: &'static str,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("openshell/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(900))
        .build()?)
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    let mut req = client.get(url);
    if url.starts_with("https://api.github.com/") {
        if let Ok(t) = std::env::var("GITHUB_TOKEN") {
            req = req.bearer_auth(t);
        }
    }
    let resp = req.send().await.with_context(|| format!("cannot reach {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    Ok(resp.json().await?)
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client.get(url).send().await.with_context(|| format!("cannot reach {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    Ok(resp.text().await?)
}

/// `sha256  file` lines (SHASUMS256.txt, SHA256SUMS).
fn checksum_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let (hash, name) = (parts.next()?, parts.next()?);
        (name.trim_start_matches('*') == file).then(|| hash.to_lowercase())
    })
}

fn wanted(requested: Option<&str>, default: &str) -> Result<Version> {
    let v = requested.unwrap_or(default);
    Version::parse(v).ok_or_else(|| anyhow!("`{v}` is not a version"))
}

/// Find the download for a runtime, version and platform (`os`: windows/macos/linux, `arch`:
/// x86_64/aarch64 as in `std::env::consts`).
pub async fn resolve(name: &str, version: Option<&str>, os: &str, arch: &str) -> Result<Download> {
    let def = runtime(name).ok_or_else(|| anyhow!("unknown runtime `{name}`"))?;
    let bundle = def.bundle.ok_or_else(|| anyhow!("{} cannot be bundled", def.title))?;
    let want = wanted(version, bundle.default_version)?;
    let client = client()?;
    match name {
        "node" => {
            let plat = match os {
                "windows" => "win",
                "macos" => "darwin",
                "linux" => "linux",
                other => bail!("no Node.js builds for {other}"),
            };
            let a = match arch {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                other => bail!("no Node.js builds for {other}"),
            };
            let index = get_json(&client, "https://nodejs.org/dist/index.json").await?;
            let release = index
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["version"].as_str())
                .find(|v| Version::parse(v).map(|p| p.starts_with(&want)).unwrap_or(false))
                .ok_or_else(|| anyhow!("no Node.js release matches `{want}`"))?
                .to_string();
            let ext = if os == "windows" { "zip" } else { "tar.gz" };
            let file = format!("node-{release}-{plat}-{a}.{ext}");
            let base = format!("https://nodejs.org/dist/{release}");
            let sums = get_text(&client, &format!("{base}/SHASUMS256.txt")).await?;
            let sha256 = checksum_for(&sums, &file)
                .ok_or_else(|| anyhow!("{file} is not listed in SHASUMS256.txt"))?;
            Ok(Download {
                version: release.trim_start_matches('v').to_string(),
                url: format!("{base}/{file}"),
                file,
                sha256,
                source: "nodejs.org",
            })
        }
        "python" => {
            let triple = match (os, arch) {
                ("windows", "x86_64") => "x86_64-pc-windows-msvc",
                ("windows", "aarch64") => "aarch64-pc-windows-msvc",
                ("macos", "aarch64") => "aarch64-apple-darwin",
                ("macos", "x86_64") => "x86_64-apple-darwin",
                ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
                ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
                (o, a) => bail!("no standalone Python builds for {o}/{a}"),
            };
            let release = get_json(
                &client,
                "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest",
            )
            .await?;
            let assets: Vec<(&str, &str)> = release["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| Some((a["name"].as_str()?, a["browser_download_url"].as_str()?)))
                .collect();
            let suffix = format!("-{triple}-install_only.tar.gz");
            let mut best: Option<(Version, &str, &str)> = None;
            for (name, url) in &assets {
                let Some(rest) = name.strip_prefix("cpython-").and_then(|r| r.strip_suffix(&suffix)) else {
                    continue;
                };
                let Some(v) = rest.split('+').next().and_then(Version::parse) else { continue };
                if v.starts_with(&want)
                    && best.as_ref().map(|(b, ..)| v.cmp_padded(b).is_gt()).unwrap_or(true)
                {
                    best = Some((v, name, url));
                }
            }
            let (v, file, url) = best.ok_or_else(|| {
                anyhow!("the latest python-build-standalone release has no Python {want} for {triple}")
            })?;
            let sums_url = assets
                .iter()
                .find(|(n, _)| *n == "SHA256SUMS")
                .map(|(_, u)| u.to_string())
                .ok_or_else(|| anyhow!("the python-build-standalone release has no SHA256SUMS"))?;
            let sums = get_text(&client, &sums_url).await?;
            let sha256 =
                checksum_for(&sums, file).ok_or_else(|| anyhow!("{file} is not listed in SHA256SUMS"))?;
            Ok(Download {
                version: v.to_string(),
                url: url.to_string(),
                file: file.to_string(),
                sha256,
                source: "python-build-standalone",
            })
        }
        "java" => {
            let o = match os {
                "windows" => "windows",
                "macos" => "mac",
                "linux" => "linux",
                other => bail!("no Temurin builds for {other}"),
            };
            let a = match arch {
                "x86_64" => "x64",
                "aarch64" => "aarch64",
                other => bail!("no Temurin builds for {other}"),
            };
            let feature = want.0[0];
            let url = format!(
                "https://api.adoptium.net/v3/assets/latest/{feature}/hotspot?architecture={a}&image_type=jre&os={o}&vendor=eclipse"
            );
            let list = get_json(&client, &url).await?;
            let asset = list
                .as_array()
                .and_then(|l| l.first())
                .ok_or_else(|| anyhow!("no Temurin JRE {feature} for {o}/{a}"))?;
            let pkg = &asset["binary"]["package"];
            let version = asset["version"]["semver"]
                .as_str()
                .or_else(|| asset["version"]["openjdk_version"].as_str())
                .unwrap_or("")
                .split('+')
                .next()
                .unwrap_or("")
                .to_string();
            Ok(Download {
                version,
                url: pkg["link"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Temurin answer without a link"))?
                    .to_string(),
                file: pkg["name"].as_str().unwrap_or("jre").to_string(),
                sha256: pkg["checksum"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Temurin answer without a checksum"))?
                    .to_lowercase(),
                source: "Eclipse Temurin (adoptium.net)",
            })
        }
        other => bail!("downloading `{other}` is not supported"),
    }
}

/// Unpack an archive into `dest`; a single top-level folder is flattened away.
fn unpack(archive: &Path, file_name: &str, dest: &Path) -> Result<()> {
    let staging = dest.with_file_name(format!(
        ".{}-unpack",
        dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    if file_name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        zip.extract(&staging).context("unpacking the zip archive")?;
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut tar = tar::Archive::new(gz);
        tar.set_preserve_permissions(true);
        tar.unpack(&staging).context("unpacking the tar.gz archive")?;
    } else {
        bail!("unsupported archive `{file_name}`");
    }
    let entries: Vec<PathBuf> =
        std::fs::read_dir(&staging)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    let root = if entries.len() == 1 && entries[0].is_dir() { entries[0].clone() } else { staging.clone() };
    if dest.exists() {
        std::fs::remove_dir_all(dest).with_context(|| format!("cannot replace {}", dest.display()))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&root, dest).with_context(|| format!("cannot move the runtime to {}", dest.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download, verify and unpack a runtime into its folder. `progress` gets short status lines.
pub async fn install(
    project: &Project,
    name: &str,
    force: bool,
    progress: impl Fn(&str),
) -> Result<Installed> {
    let entry = project.config.runtimes.get(name).ok_or_else(|| {
        anyhow!("`{name}` is not declared in `runtimes` (add it with `openshell runtime add {name}`)")
    })?;
    let dir = project.root.join(entry.path(name));
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    if let Some(have) = installed(&dir) {
        let matches = entry
            .version()
            .and_then(Version::parse)
            .zip(Version::parse(&have.version))
            .map(|(want, got)| got.starts_with(&want))
            .unwrap_or(true);
        if !force && matches && have.os == os && have.arch == arch {
            progress(&format!("{name} {} is already installed in {}", have.version, entry.path(name)));
            return Ok(have);
        }
    }
    progress(&format!("looking up {name} {}", entry.version().unwrap_or("(default version)")));
    let dl = resolve(name, entry.version(), os, arch).await?;
    progress(&format!("downloading {} from {}", dl.file, dl.source));
    let client = client()?;
    let resp = client.get(&dl.url).send().await.with_context(|| format!("cannot reach {}", dl.url))?;
    if !resp.status().is_success() {
        bail!("{} answered {}", dl.url, resp.status());
    }
    let bytes = resp.bytes().await?;
    let tmp = tempfile_in(&project.root)?;
    std::fs::write(&tmp, &bytes)?;
    let actual = sha256_file(&tmp)?;
    if actual != dl.sha256 {
        let _ = std::fs::remove_file(&tmp);
        bail!("checksum mismatch for {}: expected {}, got {actual}", dl.file, dl.sha256);
    }
    progress(&format!("checksum ok ({} MB), unpacking", bytes.len() / 1_000_000));
    let result = unpack(&tmp, &dl.file, &dir);
    let _ = std::fs::remove_file(&tmp);
    result?;
    let info = Installed {
        name: name.to_string(),
        version: dl.version,
        os: os.to_string(),
        arch: arch.to_string(),
        source: dl.source.to_string(),
        url: dl.url,
        sha256: dl.sha256,
    };
    std::fs::write(dir.join(MARKER), serde_json::to_vec_pretty(&info)?)?;
    Ok(info)
}

fn tempfile_in(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".openshell").join("downloads");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{}.part", uuid::Uuid::new_v4().simple())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to nodejs.org, GitHub and Adoptium (metadata and HEAD requests only, no archive is
    /// downloaded): `cargo test -p openshell-core runtimes -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn resolves_downloads_for_every_platform() {
        let client = client().unwrap();
        for name in ["node", "python", "java"] {
            for (os, arch) in [
                ("windows", "x86_64"),
                ("macos", "aarch64"),
                ("macos", "x86_64"),
                ("linux", "x86_64"),
                ("linux", "aarch64"),
            ] {
                let d = resolve(name, None, os, arch)
                    .await
                    .unwrap_or_else(|e| panic!("{name} {os}/{arch}: {e:#}"));
                assert_eq!(d.sha256.len(), 64, "{name} {os}/{arch}: {d:?}");
                let head = client.head(&d.url).send().await.unwrap();
                assert!(
                    head.status().is_success() || head.status().is_redirection(),
                    "{} -> {}",
                    d.url,
                    head.status()
                );
                println!("{name:<7} {os}/{arch}: {} {}", d.version, d.file);
            }
        }
        let pinned = resolve("python", Some("3.11"), "linux", "x86_64").await.unwrap();
        assert!(pinned.version.starts_with("3.11."), "{pinned:?}");
        let node20 = resolve("node", Some("20"), "windows", "x86_64").await.unwrap();
        assert!(node20.version.starts_with("20."), "{node20:?}");
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let sums = "abc123  node-v22.1.0-win-x64.zip\nDEF456 *node-v22.1.0-linux-x64.tar.gz\n";
        assert_eq!(checksum_for(sums, "node-v22.1.0-win-x64.zip").as_deref(), Some("abc123"));
        assert_eq!(checksum_for(sums, "node-v22.1.0-linux-x64.tar.gz").as_deref(), Some("def456"));
        assert_eq!(checksum_for(sums, "other"), None);
    }

    #[test]
    fn unpack_flattens_the_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("rt.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.add_directory("node-v1/", opts).unwrap();
            zip.start_file("node-v1/node.exe", opts).unwrap();
            std::io::Write::write_all(&mut zip, b"binary").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.path().join("runtimes/node");
        unpack(&archive, "rt.zip", &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("node.exe")).unwrap(), b"binary");
        assert!(!dir.path().join("runtimes/.node-unpack").exists());
        // replacing an existing runtime
        unpack(&archive, "rt.zip", &dest).unwrap();
        assert!(dest.join("node.exe").is_file());
    }
}
