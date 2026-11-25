//! Node.js: a `package.json` with `dev` / `start` script that is not the frontend, or a
//! `server.js` / `index.js` in a backend folder.

use super::{package_manager, read_json, Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "node", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    if Some(ctx.rel_dir) == ctx.frontend_dir {
        return None;
    }
    if let Some(pkg) = read_json(&ctx.dir.join("package.json")) {
        let pm = package_manager(ctx.dir);
        let cmd = if pkg["scripts"].get("dev").is_some() {
            Some(format!("{pm} run dev"))
        } else if pkg["scripts"].get("start").is_some() {
            Some(format!("{pm} start"))
        } else {
            None
        };
        if let Some(cmd) = cmd {
            return Some(Backend::new("node", ctx.rel_dir, json!(cmd), 3001, "Node.js"));
        }
    }
    for f in ["server.js", "index.js", "server.mjs"] {
        if ctx.dir.join(f).is_file() && !ctx.is_root() {
            return Some(Backend::new("node", ctx.rel_dir, json!(["node", f]), 3001, "Node.js"));
        }
    }
    None
}
