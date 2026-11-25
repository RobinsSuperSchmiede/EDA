//! C and C++: CMake and Make projects.

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "cpp", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    let dir = ctx.dir;
    if let Ok(cmake) = std::fs::read_to_string(dir.join("CMakeLists.txt")) {
        let target = cmake
            .find("add_executable(")
            .map(|i| &cmake[i + 15..])
            .and_then(|r| r.split(|c: char| c.is_whitespace() || c == ')').find(|s| !s.is_empty()))
            .unwrap_or("app")
            .to_string();
        let mut b = Backend::new("cpp", ctx.rel_dir, json!([format!("build/{target}")]), 8080, "C++ (CMake)");
        b.build = Some(json!(
            "cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build --config Release"
        ));
        b.watch = Some("src".into());
        b.note = Some(format!(
            "multi-config generators (Visual Studio) put `{target}` into build/Release/ - adjust `command` or set CMAKE_RUNTIME_OUTPUT_DIRECTORY"
        ));
        return Some(b);
    }
    if dir.join("Makefile").is_file() && (dir.join("main.cpp").exists() || dir.join("src").is_dir()) {
        let mut b = Backend::new("cpp", ctx.rel_dir, json!(["./app"]), 8080, "C/C++ (Make)");
        b.build = Some(json!("make"));
        b.note = Some("set `command` to the binary your Makefile produces".into());
        return Some(b);
    }
    None
}
