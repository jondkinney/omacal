<!-- ui/src/lib/TasksPanel.svelte -->
<script lang="ts">
  import { escapeCloses } from './dismiss.svelte';
  import {
    createTask, deleteTask, listTasks, setTaskCompleted, taskLists,
    type Task, type TaskList,
  } from './tasks';

  let { onclose }: { onclose: () => void } = $props();

  let tasks = $state<Task[] | null>(null);
  let lists = $state<TaskList[]>([]);
  let note = $state<string | null>(null);
  /** Rows with a toggle in flight — their checkbox freezes rather than lies. */
  let busyIds = $state<Set<number>>(new Set());

  let newTitle = $state('');
  let newListId = $state<number | null>(null);
  let adding = $state(false);

  $effect(() => {
    void (async () => {
      try {
        const [t, l] = await Promise.all([listTasks(), taskLists()]);
        tasks = t;
        lists = l;
        if (newListId === null && l.length > 0) newListId = l[0].calendarId;
      } catch (e) {
        note = String(e);
        tasks = [];
      }
    })();
  });

  escapeCloses(() => true, onclose);

  const open = $derived((tasks ?? []).filter((t) => !t.completed));
  const done = $derived((tasks ?? []).filter((t) => t.completed));

  async function toggle(task: Task) {
    if (busyIds.has(task.id)) return;
    note = null;
    busyIds = new Set([...busyIds, task.id]);
    try {
      tasks = await setTaskCompleted(task.id, !task.completed);
    } catch (e) {
      note = String(e);
    } finally {
      const next = new Set(busyIds);
      next.delete(task.id);
      busyIds = next;
    }
  }

  async function remove(task: Task) {
    note = null;
    try {
      tasks = await deleteTask(task.id);
    } catch (e) {
      note = String(e);
    }
  }

  async function add() {
    if (adding || newListId === null || newTitle.trim() === '') return;
    note = null;
    adding = true;
    try {
      tasks = await createTask(newListId, newTitle, null);
      newTitle = '';
    } catch (e) {
      note = String(e);
    } finally {
      adding = false;
    }
  }

  /** "today" / "Mon 18 Aug" / "overdue" — glanceable, not a timestamp. */
  function dueLabel(t: Task): string {
    if (t.dueMs === null) return '';
    const due = new Date(t.dueMs);
    const now = new Date();
    const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const diff = Math.round((day(due) - day(now)) / 86400000);
    if (diff < 0 && !t.completed) return 'overdue';
    if (diff === 0) return 'today';
    if (diff === 1) return 'tomorrow';
    return due.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="panel" role="dialog" aria-label="Tasks">
  <header>
    <h2>Tasks</h2>
    <button class="close" aria-label="Close" onclick={onclose}>×</button>
  </header>

  {#if note}
    <p class="note" role="alert">{note}</p>
  {/if}

  {#if lists.length > 0}
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <input
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        bind:value={newTitle}
        disabled={adding}
      />
      {#if lists.length > 1}
        <select aria-label="Task list" bind:value={newListId} disabled={adding}>
          {#each lists as l (l.calendarId)}
            <option value={l.calendarId}>{l.name}</option>
          {/each}
        </select>
      {/if}
    </form>
  {/if}

  {#if tasks === null}
    <p class="empty">Loading…</p>
  {:else if tasks.length === 0}
    <p class="empty">
      No tasks yet. Task lists arrive with an iCloud or CalDAV account
      (Settings → Accounts).
    </p>
  {:else}
    <ul>
      {#each open as t (t.id)}
        <li>
          <input
            type="checkbox"
            checked={false}
            disabled={!t.canWrite || busyIds.has(t.id)}
            aria-label="Complete {t.summary}"
            onchange={() => toggle(t)}
          />
          <span class="tick" style:background={t.color ?? 'var(--muted)'}></span>
          <span class="title">{t.summary}</span>
          <span class="due" class:overdue={dueLabel(t) === 'overdue'}>{dueLabel(t)}</span>
          {#if t.canWrite}
            <button class="del" aria-label="Delete {t.summary}" onclick={() => remove(t)}>×</button>
          {/if}
        </li>
      {/each}
      {#if done.length > 0}
        <li class="section" aria-hidden="true">Done</li>
        {#each done as t (t.id)}
          <li class="done">
            <input
              type="checkbox"
              checked={true}
              disabled={!t.canWrite || busyIds.has(t.id)}
              aria-label="Reopen {t.summary}"
              onchange={() => toggle(t)}
            />
            <span class="tick" style:background={t.color ?? 'var(--muted)'}></span>
            <span class="title">{t.summary}</span>
          </li>
        {/each}
      {/if}
    </ul>
  {/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 40; background: transparent; border: 0; }
  .panel {
    position: fixed; top: 46px; right: 12px; z-index: 41;
    width: 320px; max-height: min(70vh, 560px); overflow-y: auto;
    background: var(--surface); border: 1px solid var(--hairline); border-radius: 8px;
    padding: 10px 12px 12px; box-shadow: 0 8px 28px rgb(0 0 0 / 0.35);
    display: flex; flex-direction: column; gap: 8px;
  }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { font-size: 13px; margin: 0; letter-spacing: 0.02em; }
  .close { font: inherit; font-size: 15px; color: var(--muted); background: none;
    border: 0; cursor: pointer; padding: 0 2px; }
  .close:hover { color: var(--text); }

  .note { font-size: 11.5px; color: var(--danger, #e66); margin: 0; }

  .add { display: flex; gap: 6px; }
  .add input, .add select {
    font: inherit; font-size: 12.5px; color: var(--text); background: var(--bg);
    border: 1px solid var(--hairline); border-radius: 5px; padding: 5px 8px; min-width: 0;
  }
  .add input { flex: 1; }
  .add :is(input, select):focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }

  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  li { display: flex; align-items: center; gap: 8px; padding: 4px 2px; font-size: 12.5px; }
  li.section { color: var(--muted); font-size: 11px; letter-spacing: 0.06em;
    text-transform: uppercase; margin-top: 6px; }
  .tick { width: 3px; height: 16px; border-radius: 1.5px; flex: none; }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li.done .title { color: var(--muted); text-decoration: line-through; }
  .due { color: var(--muted); font-size: 11px; flex: none; }
  .due.overdue { color: var(--danger, #e66); }
  .del { font: inherit; color: var(--muted); background: none; border: 0; cursor: pointer;
    padding: 0 2px; visibility: hidden; }
  li:hover .del { visibility: visible; }
  .del:hover { color: var(--text); }
  .empty { font-size: 12px; color: var(--muted); margin: 0; line-height: 1.5; }
</style>
