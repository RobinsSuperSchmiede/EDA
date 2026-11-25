//! Go: a module with `go.mod`.

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "go", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    ctx.dir.join("go.mod").is_file().then(|| {
        let mut b = Backend::new("go", ctx.rel_dir, json!("go run ."), 8080, "Go");
        b.note = Some(
            "for installers, build a binary (`go build -o bin/server`) and run that instead of `go run`"
                .into(),
        );
        b
    })
}
