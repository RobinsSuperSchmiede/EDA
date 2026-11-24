//! Address allowlist, rate limit and lockout after failed logins for the remote gateway and
//! exposed services.

use openshell_config::{cidr_contains, parse_cidr, RateLimitConfig};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub type Range = (IpAddr, u8);

/// Why a connection or request was turned away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The address is not in the allowlist.
    NotAllowed,
    /// Blocked after too many failed logins.
    Blocked,
    /// More requests per minute than allowed.
    RateLimited,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::NotAllowed => "this address is not allowed to use the remote gateway",
            Refusal::Blocked => "too many failed logins from this address; try again later",
            Refusal::RateLimited => "too many requests; slow down",
        }
    }
}

/// Parse an allowlist (validated before, invalid entries are skipped).
pub fn ranges(list: &[String]) -> Vec<Range> {
    list.iter().filter_map(|s| parse_cidr(s)).collect()
}

/// Whether `ip` may connect. An empty list allows everyone; loopback is always allowed so the
/// host itself can test the gateway.
pub fn allowed(ranges: &[Range], ip: IpAddr) -> bool {
    ranges.is_empty() || ip.is_loopback() || ranges.iter().any(|r| cidr_contains(*r, ip))
}

#[derive(Default)]
struct Client {
    window_start: Option<Instant>,
    requests: u32,
    failures: u32,
    blocked_until: Option<Instant>,
}

/// Per-address counters (in memory; a restart clears them).
pub struct AccessControl {
    limits: RateLimitConfig,
    clients: Mutex<HashMap<IpAddr, Client>>,
}

const WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED: usize = 10_000;

impl AccessControl {
    pub fn new(limits: RateLimitConfig) -> Self {
        Self { limits, clients: Mutex::new(HashMap::new()) }
    }

    /// Admit a new request or connection from `ip`: checks the allowlist, an active block and
    /// the rate limit, and counts the request.
    pub fn admit(&self, ranges: &[Range], ip: IpAddr) -> Result<(), Refusal> {
        self.admit_at(ranges, ip, Instant::now())
    }

    fn admit_at(&self, ranges: &[Range], ip: IpAddr, now: Instant) -> Result<(), Refusal> {
        if !allowed(ranges, ip) {
            return Err(Refusal::NotAllowed);
        }
        let mut clients = self.clients.lock().unwrap();
        if clients.len() > MAX_TRACKED {
            clients.retain(|_, c| {
                c.blocked_until.is_some_and(|t| t > now)
                    || c.window_start.is_some_and(|t| now.duration_since(t) < WINDOW)
            });
        }
        let c = clients.entry(ip).or_default();
        if let Some(until) = c.blocked_until {
            if until > now {
                return Err(Refusal::Blocked);
            }
            c.blocked_until = None;
            c.failures = 0;
        }
        let limit = self.limits.requests_per_minute;
        if limit == 0 {
            return Ok(());
        }
        match c.window_start {
            Some(start) if now.duration_since(start) < WINDOW => {
                if c.requests >= limit {
                    return Err(Refusal::RateLimited);
                }
                c.requests += 1;
            }
            _ => {
                c.window_start = Some(now);
                c.requests = 1;
            }
        }
        Ok(())
    }

    /// Record a failed login. Returns `true` when this failure blocked the address.
    pub fn failed_login(&self, ip: IpAddr) -> bool {
        self.failed_login_at(ip, Instant::now())
    }

    fn failed_login_at(&self, ip: IpAddr, now: Instant) -> bool {
        let max = self.limits.max_failed_auth;
        if max == 0 || ip.is_loopback() {
            return false;
        }
        let mut clients = self.clients.lock().unwrap();
        let c = clients.entry(ip).or_default();
        c.failures += 1;
        if c.failures >= max && self.limits.block_minutes > 0 {
            c.blocked_until = Some(now + Duration::from_secs(self.limits.block_minutes * 60));
            c.failures = 0;
            return true;
        }
        false
    }

    /// A successful login resets the failure counter.
    pub fn logged_in(&self, ip: IpAddr) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&ip) {
            c.failures = 0;
        }
    }

    /// Addresses blocked right now (for `openshell remote`).
    pub fn blocked(&self) -> Vec<IpAddr> {
        let now = Instant::now();
        self.clients
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| c.blocked_until.is_some_and(|t| t > now))
            .map(|(ip, _)| *ip)
            .collect()
    }

    /// Lift a block (admin API).
    pub fn unblock(&self, ip: IpAddr) -> bool {
        match self.clients.lock().unwrap().get_mut(&ip) {
            Some(c) if c.blocked_until.is_some() => {
                c.blocked_until = None;
                c.failures = 0;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn allowlist() {
        let r = ranges(&["192.168.1.0/24".into(), "100.64.0.0/10".into()]);
        assert!(allowed(&r, ip("192.168.1.20")));
        assert!(allowed(&r, ip("100.100.1.1")));
        assert!(allowed(&r, ip("127.0.0.1")), "loopback is always allowed");
        assert!(!allowed(&r, ip("10.0.0.1")));
        assert!(allowed(&[], ip("8.8.8.8")), "empty list allows everyone");
        let ac = AccessControl::new(RateLimitConfig::default());
        assert_eq!(ac.admit(&r, ip("10.0.0.1")), Err(Refusal::NotAllowed));
    }

    #[test]
    fn rate_limit_per_minute() {
        let ac = AccessControl::new(RateLimitConfig { requests_per_minute: 3, ..Default::default() });
        let t = Instant::now();
        let a = ip("192.168.1.2");
        for _ in 0..3 {
            assert!(ac.admit_at(&[], a, t).is_ok());
        }
        assert_eq!(ac.admit_at(&[], a, t), Err(Refusal::RateLimited));
        assert!(ac.admit_at(&[], ip("192.168.1.3"), t).is_ok(), "limits are per address");
        assert!(ac.admit_at(&[], a, t + Duration::from_secs(61)).is_ok(), "new window");
    }

    #[test]
    fn lockout_after_failed_logins() {
        let ac = AccessControl::new(RateLimitConfig {
            requests_per_minute: 0,
            max_failed_auth: 3,
            block_minutes: 1,
        });
        let t = Instant::now();
        let a = ip("10.1.1.1");
        assert!(!ac.failed_login_at(a, t));
        assert!(!ac.failed_login_at(a, t));
        assert!(ac.failed_login_at(a, t), "third failure blocks");
        assert_eq!(ac.admit_at(&[], a, t + Duration::from_secs(30)), Err(Refusal::Blocked));
        assert!(ac.admit_at(&[], a, t + Duration::from_secs(61)).is_ok(), "block expires");
        assert!(!ac.failed_login_at(ip("127.0.0.1"), t), "loopback is never blocked");

        assert!(ac.failed_login_at(a, t) || ac.failed_login_at(a, t) || ac.failed_login_at(a, t));
        assert_eq!(ac.blocked(), vec![a]);
        assert!(ac.unblock(a));
        assert!(ac.blocked().is_empty());
    }

    #[test]
    fn successful_login_resets_failures() {
        let ac = AccessControl::new(RateLimitConfig { max_failed_auth: 2, ..Default::default() });
        let a = ip("10.1.1.2");
        assert!(!ac.failed_login(a));
        ac.logged_in(a);
        assert!(!ac.failed_login(a), "counter was reset");
    }
}
