# @openshell/sdk

Optional frontend SDK for applications running in [OpenShell](https://github.com/RobinsSuperSchmiede/EDA).
No dependencies, no build step, works with any framework. Outside OpenShell `OpenShell.available` is `false`.

```js
import { OpenShell } from "@openshell/sdk";

if (OpenShell.available) {
  const { services } = await OpenShell.runtime.status();
  await OpenShell.notify("Sync finished");
  OpenShell.agent.registerAction("refresh_dashboard", { description: "Reload dashboard data" }, () => reload());
}
```

Every running shell also serves this file at `{control_url}/sdk.js`. Full documentation: [docs/sdk.md](../../docs/sdk.md).
