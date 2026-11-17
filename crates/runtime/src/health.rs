//! Health probes (HTTP and TCP).

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// HTTP GET. `accept_any`: any HTTP response counts (used for dev servers
    /// whose root may return 404); otherwise 2xx/3xx is required.
    Http { url: String, accept_any: bool },
    /// TCP connect to `host:port`.
    Tcp { addr: String },
    /// No probe: the service is considered ready shortly after it started.
    None,
}

#[derive(Debug, Clone)]
pub struct ProbeConfig {
    pub probe: Probe,
    pub interval: Duration,
    pub timeout: Duration,
    pub failure_threshold: u32,
    pub ready_timeout: Duration,
}

impl ProbeConfig {
    pub fn none() -> Self {
        Self {
            probe: Probe::None,
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(2),
            failure_threshold: 3,
            ready_timeout: Duration::from_secs(60),
        }
    }
}

/// Outcome of a single probe.
pub type ProbeResult = Result<(), String>;

pub async fn check(client: &reqwest::Client, probe: &Probe, timeout: Duration) -> ProbeResult {
    match probe {
        Probe::None => Ok(()),
        Probe::Http { url, accept_any } => {
            let resp = client
                .get(url)
                .timeout(timeout)
                .send()
                .await
                .map_err(|e| format!("GET {url}: {}", root_cause(&e)))?;
            let status = resp.status();
            if *accept_any || status.is_success() || status.is_redirection() {
                Ok(())
            } else {
                Err(format!("GET {url} returned {status}"))
            }
        }
        Probe::Tcp { addr } => {
            match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr.as_str())).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(format!("connect {addr}: {e}")),
                Err(_) => Err(format!("connect {addr}: timed out")),
            }
        }
    }
}

fn root_cause(e: &(dyn std::error::Error + 'static)) -> String {
    let mut current = e;
    while let Some(src) = current.source() {
        current = src;
    }
    current.to_string()
}
