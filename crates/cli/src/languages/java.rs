//! Java: Maven / Gradle (Spring Boot or plain) and plain `javac` sources.

use super::{Backend, Ctx, Detector};
use serde_json::json;

pub const DETECTOR: Detector = Detector { language: "java", detect };

/// First `<tag>value</tag>` outside of `<parent>` / `<dependencies>` / `<build>` blocks.
fn pom_value(pom: &str, tag: &str) -> Option<String> {
    let mut text = pom.to_string();
    for block in ["parent", "dependencies", "build", "dependencyManagement", "profiles"] {
        let (open, close) = (format!("<{block}>"), format!("</{block}>"));
        while let (Some(a), Some(b)) = (text.find(&open), text.find(&close)) {
            if b < a {
                break;
            }
            text.replace_range(a..b + close.len(), "");
        }
    }
    let start = text.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = text[start..].find('<')? + start;
    Some(text[start..end].trim().to_string())
}

fn detect(ctx: &Ctx) -> Option<Backend> {
    let dir = ctx.dir;
    if let Ok(pom) = std::fs::read_to_string(dir.join("pom.xml")) {
        let mvn = if dir.join("mvnw").exists() || dir.join("mvnw.cmd").exists() { "mvnw" } else { "mvn" };
        if pom.contains("spring-boot") {
            let mut b = Backend::new(
                "java",
                ctx.rel_dir,
                json!([mvn, "-q", "spring-boot:run", "-Dspring-boot.run.arguments=--server.port=${port}"]),
                8080,
                "Java (Spring Boot, Maven)",
            );
            b.healthcheck =
                pom.contains("actuator").then(|| "http://127.0.0.1:${port}/actuator/health".into());
            b.openapi = pom.contains("springdoc").then(|| "/v3/api-docs".into());
            b.watch = Some("src".into());
            return Some(b);
        }
        let artifact = pom_value(&pom, "artifactId").unwrap_or_else(|| "app".into());
        let version = pom_value(&pom, "version").unwrap_or_else(|| "1.0-SNAPSHOT".into());
        let mut b = Backend::new(
            "java",
            ctx.rel_dir,
            json!(["java", "-jar", format!("target/{artifact}-{version}.jar")]),
            8080,
            "Java (Maven)",
        );
        b.build = Some(json!([mvn, "-q", "-DskipTests", "package"]));
        b.watch = Some("src".into());
        b.note = Some("check that the jar name in `command` matches your build output".into());
        return Some(b);
    }
    for gradle in ["build.gradle", "build.gradle.kts"] {
        if let Ok(text) = std::fs::read_to_string(dir.join(gradle)) {
            let g = if dir.join("gradlew").exists() || dir.join("gradlew.bat").exists() {
                "gradlew"
            } else {
                "gradle"
            };
            let spring = text.contains("org.springframework.boot");
            let mut b = Backend::new(
                "java",
                ctx.rel_dir,
                if spring {
                    json!([g, "-q", "bootRun", "--args=--server.port=${port}"])
                } else {
                    json!([g, "-q", "run"])
                },
                8080,
                if spring { "Java (Spring Boot, Gradle)" } else { "Java (Gradle)" },
            );
            b.openapi = (spring && text.contains("springdoc")).then(|| "/v3/api-docs".into());
            b.watch = Some("src".into());
            return Some(b);
        }
    }
    // plain sources: compile with javac
    for main in ["src/Main.java", "Main.java"] {
        if dir.join(main).is_file() {
            let mut b = Backend::new(
                "java",
                ctx.rel_dir,
                json!(["java", "-cp", "build/classes", "Main"]),
                8000,
                "Java (javac)",
            );
            b.build = Some(json!(["javac", "-d", "build/classes", main]));
            b.watch = Some(if main.starts_with("src") { "src".into() } else { main.into() });
            return Some(b);
        }
    }
    None
}
