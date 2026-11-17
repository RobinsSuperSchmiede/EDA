<script setup>
import { computed, onMounted, onUnmounted, ref } from "vue";
import { api, openshell } from "./api.js";

const todos = ref([]);
const title = ref("");
const filter = ref("all");
const error = ref(null);
let unregister;

const refresh = () => api("/api/todos").then((t) => (todos.value = t), (e) => (error.value = e));
const visible = computed(() => todos.value.filter((t) => filter.value === "all" || (filter.value === "done" ? t.done : !t.done)));
const done = computed(() => todos.value.filter((t) => t.done).length);

async function add() {
  if (!title.value.trim()) return;
  await api("/api/todos", { method: "POST", body: JSON.stringify({ title: title.value }) }).catch((e) => (error.value = e));
  title.value = "";
  refresh();
}
const toggle = (id) => api(`/api/todos/${id}/toggle`, { method: "POST" }).then(refresh);
const remove = (id) => api(`/api/todos/${id}`, { method: "DELETE" }).then(refresh);

onMounted(async () => {
  refresh();
  const os = await openshell();
  unregister = os?.agent.registerAction(
    "set_filter",
    {
      description: "Show all, only open or only done todos in the UI",
      permission: "ui",
      input_schema: { type: "object", properties: { filter: { enum: ["all", "open", "done"] } }, required: ["filter"] },
    },
    ({ filter: f }) => ((filter.value = f), { filter: f }),
  );
});
onUnmounted(() => unregister?.());
</script>

<template>
  <main>
    <h1>{{app_name}}</h1>
    <p class="muted">{{ todos.length }} todos · {{ done }} done · {{ todos.length - done }} open</p>
    <form @submit.prevent="add">
      <input v-model="title" aria-label="New todo" placeholder="What needs to be done?" />
      <button type="submit">Add</button>
    </form>
    <nav aria-label="Filter">
      <button v-for="f in ['all', 'open', 'done']" :key="f" type="button" :aria-pressed="filter === f" @click="filter = f">
        {{ f[0].toUpperCase() + f.slice(1) }}
      </button>
    </nav>
    <ul>
      <li v-for="t in visible" :key="t.id" :class="{ done: t.done }">
        <label><input type="checkbox" :checked="t.done" @change="toggle(t.id)" /> {{ t.title }}</label>
        <button type="button" class="delete" :aria-label="`Delete ${t.title}`" @click="remove(t.id)">×</button>
      </li>
    </ul>
    <p v-if="error" role="alert">{{ String(error.message || error) }}</p>
  </main>
</template>
