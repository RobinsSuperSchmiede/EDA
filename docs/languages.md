# Languages: C++, Java and others

OpenShell runs any executable, so every language works. Compiled languages use a **`build`** step:

- in **development** mode the build runs before every start – and therefore after every file change in `watch`,
  so editing `src/` recompiles and restarts the service automatically,
- `openshell build` / `openshell package` run all builds for production,
- a failing build marks the service `failed` with the compiler output in the logs (also readable by agents via
  `logs_read {"errors_only": true}`); fixing the code retries automatically.

```json
"build": "cmake --build build"                                       // shell string
"build": ["javac", "-d", "build/classes", "src/Main.java"]           // argv
"build": { "command": "mvn -q package", "cwd": "backend", "on_start": "never" }   // development (default) | always | never
```

Templates: `openshell create my-app -t vanilla-java` and `-t vanilla-cpp`. `openshell init` detects Maven, Gradle,
Spring Boot, plain `javac` sources, CMake and Makefiles.

## Java

### Plain JDK (template `vanilla-java`)

```json
{
  "name": "backend",
  "cwd": "backend",
  "build": ["javac", "-d", "build/classes", "src/Main.java"],
  "command": ["java", "-cp", "build/classes", "Main"],
  "port": "auto",
  "healthcheck": "http://127.0.0.1:${port}/health",
  "watch": ["backend/src"]
}
```

### Spring Boot

```json
{
  "name": "backend",
  "cwd": "backend",
  "command": ["mvnw", "-q", "spring-boot:run", "-Dspring-boot.run.arguments=--server.port=${port}"],
  "port": "auto",
  "healthcheck": { "http": "http://127.0.0.1:${port}/actuator/health", "ready_timeout_ms": 120000 },
  "openapi": "/v3/api-docs",
  "watch": ["backend/src"]
}
```

`mvnw` / `gradlew` are found in the service directory on all platforms. With
[springdoc-openapi](https://springdoc.org) the `openapi` field turns every REST endpoint into an agent tool.
For production use the jar:

```json
"build": { "command": ["mvnw", "-q", "-DskipTests", "package"], "on_start": "never" },
"command": ["java", "-jar", "target/app-1.0.0.jar", "--server.port=${port}"]
```

Gradle: `["gradlew", "-q", "bootRun", "--args=--server.port=${port}"]`.

Java UI apps (Swing/JavaFX) cannot be embedded into the WebView; run them as a service if they offer an HTTP API, or
use OpenShell for their web front end.

## C++

### CMake (template `vanilla-cpp`)

```json
{
  "name": "backend",
  "cwd": "backend",
  "build": "cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build --config Release",
  "command": ["build/bin/todo-server"],
  "port": "auto",
  "watch": ["backend/src", "backend/CMakeLists.txt"]
}
```

- `command` paths without extension also find `name.exe` on Windows.
- Multi-config generators (Visual Studio) put binaries into `build/Release/` – the template sets
  `CMAKE_RUNTIME_OUTPUT_DIRECTORY` so the path is `build/bin` everywhere.
- On Windows run OpenShell from a *Developer PowerShell* (or install CMake + Ninja / MinGW) so the compiler is found.
- Ship only the binary: `"package": { "include": ["openshell.json", "web", "backend/build/bin"] }`.
- HTTP libraries such as cpp-httplib, Crow, Drogon or Boost.Beast work the same way.

## Others

| Language | `build` | `command` |
|---|---|---|
| Rust | `cargo build --release` | `["target/release/server"]` |
| Go | `go build -o bin/server .` | `["bin/server"]` (or `go run .` without build) |
| C# / .NET | `dotnet build -c Release` | `["dotnet", "run", "--no-build", "-c", "Release", "--", "--urls", "http://127.0.0.1:${port}"]` |
| Kotlin (Ktor) | – | `["gradlew", "-q", "run"]` |
| Python | – (`setup` for `pip install`) | `["python", "main.py"]` |
| Node.js / Deno / Bun | – | `["node", "server.js"]`, `["deno", "run", "-A", "main.ts"]` |

ASP.NET (Swashbuckle), FastAPI, NestJS and Spring all publish OpenAPI documents – set `openapi` and agents can use
the backend without UI automation.
