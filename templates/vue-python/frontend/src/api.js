export async function api(path, options = {}) {
  const res = await fetch(path, { headers: { "Content-Type": "application/json" }, ...options });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || `HTTP ${res.status}`);
  return body;
}

/** Load the OpenShell SDK when running inside OpenShell (null in a plain browser). */
export async function openshell() {
  if (!window.__OPENSHELL__) return null;
  const { OpenShell } = await import(/* @vite-ignore */ `${window.__OPENSHELL__.api}/sdk.js`);
  return OpenShell;
}
