import { useEffect, useState } from "react";
import { api, openshell } from "./api.js";

export default function App() {
  const [todos, setTodos] = useState([]);
  const [title, setTitle] = useState("");
  const [filter, setFilter] = useState("all");
  const [error, setError] = useState(null);

  const refresh = () => api("/api/todos").then(setTodos, setError);

  useEffect(() => {
    refresh();
    // expose a UI function to AI agents (optional)
    let unregister;
    openshell().then((os) => {
      unregister = os?.agent.registerAction(
        "set_filter",
        {
          description: "Show all, only open or only done todos in the UI",
          permission: "ui",
          input_schema: { type: "object", properties: { filter: { enum: ["all", "open", "done"] } }, required: ["filter"] },
        },
        ({ filter }) => {
          setFilter(filter);
          return { filter };
        },
      );
    });
    return () => unregister?.();
  }, []);

  async function add(e) {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      await api("/api/todos", { method: "POST", body: JSON.stringify({ title }) });
      setTitle("");
      refresh();
    } catch (err) {
      setError(err);
    }
  }

  const toggle = (id) => api(`/api/todos/${id}/toggle`, { method: "POST" }).then(refresh, setError);
  const remove = (id) => api(`/api/todos/${id}`, { method: "DELETE" }).then(refresh, setError);
  const visible = todos.filter((t) => filter === "all" || (filter === "done" ? t.done : !t.done));
  const done = todos.filter((t) => t.done).length;

  return (
    <main>
      <h1>{{app_name}}</h1>
      <p className="muted">
        {todos.length} todos · {done} done · {todos.length - done} open
      </p>
      <form onSubmit={add}>
        <input aria-label="New todo" placeholder="What needs to be done?" value={title} onChange={(e) => setTitle(e.target.value)} />
        <button type="submit">Add</button>
      </form>
      <nav aria-label="Filter">
        {["all", "open", "done"].map((f) => (
          <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}>
            {f[0].toUpperCase() + f.slice(1)}
          </button>
        ))}
      </nav>
      <ul>
        {visible.map((t) => (
          <li key={t.id} className={t.done ? "done" : ""}>
            <label>
              <input type="checkbox" checked={t.done} onChange={() => toggle(t.id)} /> {t.title}
            </label>
            <button type="button" className="delete" aria-label={`Delete ${t.title}`} onClick={() => remove(t.id)}>
              ×
            </button>
          </li>
        ))}
      </ul>
      {error && <p role="alert">{String(error.message || error)}</p>}
    </main>
  );
}
