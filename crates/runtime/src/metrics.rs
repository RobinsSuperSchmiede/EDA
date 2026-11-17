//! Resource monitoring: CPU and memory of each service's process tree.

use crate::{DefKind, Inner};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Weak;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// CPU usage of the whole process tree in percent of one core (can exceed 100 on multi-core).
    pub cpu_percent: f32,
    /// Resident memory of the whole process tree in bytes.
    pub memory_bytes: u64,
    /// Number of processes in the tree (e.g. `cmd` -> `npm` -> `node`).
    pub processes: u32,
}

const INTERVAL: Duration = Duration::from_secs(2);

/// Periodically sample all service process trees until the runtime is dropped.
pub(crate) async fn sample(inner: Weak<Inner>) {
    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    loop {
        tokio::time::sleep(INTERVAL).await;
        let Some(rt) = inner.upgrade() else { return };
        let roots: Vec<(usize, u32)> = rt
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.def.kind, DefKind::Process { .. }))
            .filter_map(|(i, s)| s.status.lock().unwrap().pid.map(|p| (i, p)))
            .collect();
        if roots.is_empty() {
            continue;
        }
        // refreshing all processes is needed to discover children; done off the async threads
        let (s, result) = tokio::task::spawn_blocking(move || {
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
            for (pid, p) in sys.processes() {
                if let Some(parent) = p.parent() {
                    children.entry(parent).or_default().push(*pid);
                }
            }
            let result: Vec<(usize, Option<Metrics>)> = roots
                .iter()
                .map(|(i, root)| {
                    let root = Pid::from_u32(*root);
                    if sys.process(root).is_none() {
                        return (*i, None);
                    }
                    let mut m = Metrics { cpu_percent: 0.0, memory_bytes: 0, processes: 0 };
                    let mut stack = vec![root];
                    let mut seen = std::collections::HashSet::new();
                    while let Some(pid) = stack.pop() {
                        if !seen.insert(pid) {
                            continue;
                        }
                        if let Some(p) = sys.process(pid) {
                            m.cpu_percent += p.cpu_usage();
                            m.memory_bytes += p.memory();
                            m.processes += 1;
                        }
                        if let Some(c) = children.get(&pid) {
                            stack.extend(c.iter().copied());
                        }
                    }
                    m.cpu_percent = (m.cpu_percent * 10.0).round() / 10.0;
                    (*i, Some(m))
                })
                .collect();
            (sys, result)
        })
        .await
        .unwrap_or_else(|_| (System::new(), vec![]));
        sys = s;
        for (i, m) in result {
            if let Some(slot) = rt.slots.get(i) {
                let mut st = slot.status.lock().unwrap();
                if st.pid.is_some() {
                    st.metrics = m;
                }
            }
        }
    }
}
