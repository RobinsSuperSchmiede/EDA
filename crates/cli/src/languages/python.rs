//! Python: a `main.py`, `app.py` or `server.py` that serves HTTP.

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "python", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    for f in ["main.py", "app.py", "server.py"] {
        if ctx.dir.join(f).is_file() {
            let mut b = Backend::new("python", ctx.rel_dir, json!(["python", f]), 8000, "Python");
            if ctx.dir.join("requirements.txt").is_file() {
                b.note = Some(
                    "install the dependencies from requirements.txt as a `setup` step, e.g. `python -m pip install -r requirements.txt`"
                        .into(),
                );
            }
            return Some(b);
        }
    }
    None
}
