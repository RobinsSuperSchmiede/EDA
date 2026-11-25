//! Rust: a Cargo project in a backend folder (a `Cargo.toml` in the root is usually the app itself).

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "rust", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    (ctx.dir.join("Cargo.toml").is_file() && !ctx.is_root()).then(|| {
        let mut b = Backend::new("rust", ctx.rel_dir, json!("cargo run --release"), 8080, "Rust");
        b.watch = Some("src".into());
        b
    })
}
