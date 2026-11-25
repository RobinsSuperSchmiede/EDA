//! .NET: a folder with a `*.csproj` / `*.fsproj` (ASP.NET Core reads `--urls`).

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "dotnet", detect };

fn detect(ctx: &Ctx) -> Option<Backend> {
    let project = std::fs::read_dir(ctx.dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("csproj" | "fsproj")))?;
    let text = std::fs::read_to_string(&project).unwrap_or_default();
    let mut b = Backend::new(
        "dotnet",
        ctx.rel_dir,
        json!(["dotnet", "run", "--urls", "http://127.0.0.1:${port}"]),
        5000,
        if text.contains("Microsoft.NET.Sdk.Web") || text.contains("AspNetCore") {
            ".NET (ASP.NET Core)"
        } else {
            ".NET"
        },
    );
    b.openapi = (text.contains("Swashbuckle") || text.contains("Microsoft.AspNetCore.OpenApi"))
        .then(|| "/swagger/v1/swagger.json".into());
    b.watch = Some(".".into());
    b.note = Some(
        "for installers, publish self-contained (`dotnet publish -c Release --self-contained`) and run the published program"
            .into(),
    );
    Some(b)
}
