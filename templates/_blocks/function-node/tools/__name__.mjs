// Function `{{function_name}}` of {{app_name}}.
//
// AI agents call it as an MCP tool (`openshell mcp --app`), the app can call it too.
// No server is involved: OpenShell runs this script for every call.
//
//   input:  a JSON object on stdin (also in the OPENSHELL_INPUT environment variable)
//   output: JSON (or text) on stdout
//   errors: a message on stderr and a non-zero exit code
//
// Available environment: OPENSHELL_DATA_DIR, OPENSHELL_DB_<NAME> (database paths), OPENSHELL_MODE.
// Describe the function and its input in openshell.json (`capabilities`) – agents read that.
// Test it: openshell call cap_{{function_name}} '{"name": "Ada"}'

async function main(args) {
  const name = args.name || "world";
  return { message: `Hello, ${name}!` };
}

let input = "";
for await (const chunk of process.stdin) input += chunk;
const result = await main(input.trim() ? JSON.parse(input) : {});
process.stdout.write(JSON.stringify(result));
