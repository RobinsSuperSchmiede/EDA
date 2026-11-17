<script>
  import { onMount } from "svelte";
  import { api, openshell } from "./api.js";

  let todos = $state([]);
  let title = $state("");
  let filter = $state("all");
  let error = $state(null);

  const refresh = () => api("/api/todos").then((t) => (todos = t), (e) => (error = e));
  const visible = $derived(todos.filter((t) => filter === "all" || (filter === "done" ? t.done : !t.done)));
  const done = $derived(todos.filter((t) => t.done).length);

  async function add(e) {
    e.preventDefault();
    if (!title.trim()) return;
    await api("/api/todos", { method: "POST", body: JSON.stringify({ title }) }).catch((err) => (error = err));
    title = "";
    refresh();
  }
  const toggle = (id) => api(`/api/todos/${id}/toggle`, { method: "POST" }).then(refresh);
  const remove = (id) => api(`/api/todos/${id}`, { method: "DELETE" }).then(refresh);

  onMount(() => {
    refresh();
    let unregister;
    openshell().then((os) => {
      unregister = os?.agent.registerAction(
        "set_filter",
        {
          description: "Show all, only open or only done todos in the UI",
          permission: "ui",
          input_schema: { type: "object", properties: { filter: { enum: ["all", "open", "done"] } }, required: ["filter"] },
        },
        ({ filter: f }) => ((filter = f), { filter: f }),
      );
    });
    return () => unregister?.();
  });
</script>

<main>
  <h1>{{app_name}}</h1>
  <p class="muted">{todos.length} todos · {done} done · {todos.length - done} open</p>
  <form onsubmit={add}>
    <input bind:value={title} aria-label="New todo" placeholder="What needs to be done?" />
    <button type="submit">Add</button>
  </form>
  <nav aria-label="Filter">
    {#each ["all", "open", "done"] as f}
      <button type="button" aria-pressed={filter === f} onclick={() => (filter = f)}>{f[0].toUpperCase() + f.slice(1)}</button>
    {/each}
  </nav>
  <ul>
    {#each visible as t (t.id)}
      <li class:done={t.done}>
        <label><input type="checkbox" checked={t.done} onchange={() => toggle(t.id)} /> {t.title}</label>
        <button type="button" class="delete" aria-label={`Delete ${t.title}`} onclick={() => remove(t.id)}>×</button>
      </li>
    {/each}
  </ul>
  {#if error}<p role="alert">{String(error.message || error)}</p>{/if}
</main>
