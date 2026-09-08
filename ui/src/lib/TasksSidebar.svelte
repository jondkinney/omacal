<!-- ui/src/lib/TasksSidebar.svelte -->
<script lang="ts">
  import { dateFormat } from './date.svelte';
  import { clockFormat } from './clock.svelte';
  import {
    createTask, deleteTask, listTasks, setTaskCompleted, taskLists, updateTask,
    type Task, type TaskList,
  } from './tasks';
  import {
    dateInputValue, dueFromInputs, dueLabel, isOverdue, quickDue, timeInputValue,
    whenOf, WHEN_LABEL, WHEN_ORDER, type When,
  } from './taskdates';

  /** The tasks list, beside the calendar rather than over it.
   *
   *  One control at the top does one job: it regroups the rows and nothing
   *  else. "By when" answers what needs doing now; "By list" mirrors the
   *  calendar list. Both are readings of the same tasks, so switching never
   *  changes which tasks are here — only the headings they sit under.
   *
   *  A row opens in place for editing. That is the whole of what a task can
   *  be given here: a title, a due date and a note, which is exactly what a
   *  VTODO carries and this app can write back. */
  let { onclose, onchange }: {
    onclose: () => void;
    /** Told whenever the set of tasks changed, so the grid can redraw. */
    onchange?: () => void;
  } = $props();

  let tasks = $state<Task[] | null>(null);
  let lists = $state<TaskList[]>([]);
  let note = $state<string | null>(null);
  let busyIds = $state<Set<number>>(new Set());

  type Grouping = 'when' | 'list';
  let grouping = $state<Grouping>('when');

  let newTitle = $state('');
  let filterListId = $state<number | null>(null);
  let adding = $state(false);

  /** The row open for editing, and its fields while they are being typed. */
  let editingId = $state<number | null>(null);
  let draft = $state({ summary: '', date: '', time: '', notes: '' });
  let saving = $state(false);

  /** Recomputed on every load rather than ticking: the groups only move at
   *  midnight, and a list that reshuffles under a reader is worse than one
   *  that is a few hours stale until the next sync. */
  let nowMs = $state(Date.now());

  async function load() {
    try {
      const [t, l] = await Promise.all([listTasks(), taskLists()]);
      nowMs = Date.now();
      tasks = t;
      lists = l;
    } catch (e) {
      note = String(e);
      tasks = [];
    }
  }
  $effect(() => { void load(); });

  const targetList = $derived(
    filterListId === null
      ? (lists[0] ?? null)
      : (lists.find((l) => l.calendarId === filterListId) ?? null),
  );
  const open = $derived((tasks ?? []).filter((t) => !t.completed));
  const done = $derived((tasks ?? []).filter((t) => t.completed));

  /** The rows under each heading, in the order the headings appear. */
  const byWhen = $derived(
    WHEN_ORDER.map((when) => ({
      key: when as string,
      label: WHEN_LABEL[when],
      warn: when === 'overdue',
      color: null as string | null,
      rows: open
        .filter((t) => whenOf(t, nowMs) === when)
        .sort((a, b) => (a.dueMs ?? Infinity) - (b.dueMs ?? Infinity)),
    })).filter((g) => g.rows.length > 0),
  );
  const byList = $derived(
    lists.map((l) => ({
      key: `l${l.calendarId}`,
      label: l.name,
      warn: false,
      color: l.color,
      rows: open
        .filter((t) => t.calendarId === l.calendarId)
        .sort((a, b) => (a.dueMs ?? Infinity) - (b.dueMs ?? Infinity)),
    })).filter((g) => g.rows.length > 0),
  );
  const groups = $derived(grouping === 'when' ? byWhen : byList);

  const listColor = (t: Task) =>
    t.color ?? lists.find((l) => l.calendarId === t.calendarId)?.color ?? 'var(--muted)';

  async function toggle(task: Task) {
    if (busyIds.has(task.id)) return;
    note = null;
    busyIds = new Set([...busyIds, task.id]);
    try {
      tasks = await setTaskCompleted(task.id, !task.completed);
      onchange?.();
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
      if (editingId === task.id) editingId = null;
      tasks = await deleteTask(task.id);
      onchange?.();
    } catch (e) {
      note = String(e);
    }
  }

  async function add() {
    if (adding || targetList === null || newTitle.trim() === '') return;
    note = null;
    adding = true;
    try {
      tasks = await createTask(targetList.calendarId, newTitle, null);
      newTitle = '';
      onchange?.();
    } catch (e) {
      note = String(e);
    } finally {
      adding = false;
    }
  }

  function edit(task: Task) {
    if (!task.canWrite) return;
    editingId = task.id;
    draft = {
      summary: task.summary,
      date: task.dueMs === null ? '' : dateInputValue(task.dueMs),
      time: timeInputValue(task),
      notes: task.notes ?? '',
    };
  }

  /** A quick answer sets the day and clears the hour: "Tomorrow" means the
   *  day, not tomorrow-at-this-time. */
  function setQuick(kind: 'today' | 'tomorrow' | 'nextWeek') {
    draft = { ...draft, date: dateInputValue(quickDue(kind, nowMs)), time: '' };
  }

  async function save() {
    if (saving || editingId === null || draft.summary.trim() === '') return;
    saving = true;
    note = null;
    const { ms, allDay } = dueFromInputs(draft.date, draft.time);
    try {
      tasks = await updateTask(
        editingId, draft.summary, ms, allDay, draft.notes.trim() || null,
      );
      nowMs = Date.now();
      editingId = null;
      onchange?.();
    } catch (e) {
      note = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<aside class="side" aria-label="Tasks">
  <div class="top">
    <h2>Tasks</h2>
    <div class="flex"></div>
    <!-- One control, one job: it regroups the rows below and changes
         nothing else on the screen. -->
    <div class="seg" role="group" aria-label="Group tasks">
      <button class:on={grouping === 'when'} aria-pressed={grouping === 'when'}
              onclick={() => (grouping = 'when')}>By when</button>
      <button class:on={grouping === 'list'} aria-pressed={grouping === 'list'}
              onclick={() => (grouping = 'list')}>By list</button>
    </div>
    <button class="close" aria-label="Close tasks" onclick={onclose}>×</button>
  </div>

  {#if note}
    <p class="note" role="alert">{note}</p>
  {/if}

  {#if lists.length > 0}
    <form class="add" onsubmit={(e) => { e.preventDefault(); void add(); }}>
      <input
        type="text"
        placeholder={lists.length > 1 && targetList ? `Add to ${targetList.name}…` : 'Add a task…'}
        aria-label="New task title"
        bind:value={newTitle}
        disabled={adding}
      />
      {#if lists.length > 1}
        <select aria-label="Task list" bind:value={filterListId} disabled={adding}>
          <option value={null}>All lists</option>
          {#each lists as l (l.calendarId)}
            <option value={l.calendarId}>{l.name}</option>
          {/each}
        </select>
      {/if}
    </form>
  {/if}

  <div class="rows quiet-scroll">
    {#if tasks === null}
      <p class="empty">Loading…</p>
    {:else if tasks.length === 0}
      <p class="empty">
        No tasks yet. Task lists arrive with an iCloud or CalDAV account
        (Settings → Accounts).
      </p>
    {:else}
      {#each groups as g (g.key)}
        <div class="head">
          {#if g.color}<span class="tick" style:background={g.color}></span>{/if}
          <span class="hlabel" class:warn={g.warn}>{g.label}</span>
          <span class="count">{g.rows.length}</span>
        </div>
        {#each g.rows as t (t.id)}
          {#if editingId === t.id}
            <!-- The row, open where it sits. Nothing moves and nothing
                 covers the week. -->
            <div class="editor">
              <input class="etitle" aria-label="Task title" bind:value={draft.summary}
                     disabled={saving} />
              <div class="quick">
                <button onclick={() => setQuick('today')} disabled={saving}>Today</button>
                <button onclick={() => setQuick('tomorrow')} disabled={saving}>Tomorrow</button>
                <button onclick={() => setQuick('nextWeek')} disabled={saving}>Next week</button>
                <button class="clear" onclick={() => (draft = { ...draft, date: '', time: '' })}
                        disabled={saving}>Clear</button>
              </div>
              <div class="when">
                <input type="date" aria-label="Due date" bind:value={draft.date} disabled={saving} />
                <input type="time" aria-label="Due time" bind:value={draft.time}
                       disabled={saving || draft.date === ''} />
              </div>
              <textarea class="enotes" aria-label="Notes" rows="2" placeholder="Notes"
                        bind:value={draft.notes} disabled={saving}></textarea>
              <div class="eact">
                <button onclick={() => (editingId = null)} disabled={saving}>Cancel</button>
                <button class="go" onclick={() => void save()}
                        disabled={saving || draft.summary.trim() === ''}>
                  {saving ? 'Saving…' : 'Save'}
                </button>
              </div>
            </div>
          {:else}
            <div class="row">
              <input
                type="checkbox"
                checked={false}
                disabled={!t.canWrite || busyIds.has(t.id)}
                aria-label="Complete {t.summary}"
                onchange={() => toggle(t)}
              />
              <span class="tick" style:background={listColor(t)}></span>
              <button class="title" onclick={() => edit(t)} disabled={!t.canWrite}>{t.summary}</button>
              {#if t.dueMs !== null}
                <span class="due" class:overdue={isOverdue(t, nowMs)}>
                  {dueLabel(t, nowMs, clockFormat(), dateFormat())}
                </span>
              {/if}
              {#if t.canWrite}
                <button class="del" aria-label="Delete {t.summary}" onclick={() => remove(t)}>×</button>
              {/if}
            </div>
          {/if}
        {/each}
      {/each}

      {#if done.length > 0}
        <div class="head"><span class="hlabel">Done</span><span class="count">{done.length}</span></div>
        {#each done as t (t.id)}
          <div class="row done">
            <input
              type="checkbox"
              checked={true}
              disabled={!t.canWrite || busyIds.has(t.id)}
              aria-label="Reopen {t.summary}"
              onchange={() => toggle(t)}
            />
            <span class="tick" style:background={listColor(t)}></span>
            <span class="title">{t.summary}</span>
          </div>
        {/each}
      {/if}
    {/if}
  </div>
</aside>

<style>
  .side { width: 288px; flex: 0 0 288px; display: flex; flex-direction: column;
          min-height: 0; border-right: 1px solid var(--hairline); font-size: 12px; }
  .top { display: flex; align-items: center; gap: 8px; padding: 12px 10px 10px 14px; }
  h2 { margin: 0; font-size: 13px; font-weight: 600; color: var(--text); }
  .flex { flex-grow: 1; }
  .seg { display: flex; gap: 2px; background: color-mix(in srgb, var(--text) 4%, transparent);
         border-radius: 7px; padding: 2px; }
  .seg button { appearance: none; -webkit-appearance: none; font: inherit; border: 0;
                background: none; color: var(--muted); padding: 3px 9px; border-radius: 5px;
                cursor: pointer; }
  .seg button.on { background: color-mix(in srgb, var(--text) 8%, transparent);
                   color: var(--text); font-weight: 500; }
  .close { appearance: none; -webkit-appearance: none; font: inherit; font-size: 15px;
           border: 0; background: none; color: var(--muted); cursor: pointer; padding: 0 2px; }

  .note { margin: 0 12px 8px; color: var(--error); }
  .add { display: flex; gap: 6px; margin: 0 12px 10px; }
  .add input { flex-grow: 1; min-width: 0; font: inherit; padding: 7px 10px; border-radius: 7px;
               border: 1px solid var(--hairline); background: var(--surface); color: var(--text); }
  .add select { font: inherit; border-radius: 7px; border: 1px solid var(--hairline);
                background: var(--surface); color: var(--text); padding: 0 6px; max-width: 96px; }

  .rows { flex-grow: 1; overflow-y: auto; padding: 0 8px 12px; }
  .head { display: flex; align-items: center; gap: 7px; padding: 10px 6px 5px; }
  .hlabel { font-size: 10.5px; letter-spacing: .08em; text-transform: uppercase;
            color: var(--text); font-weight: 600; }
  .hlabel.warn { color: var(--error); }
  .count { font-size: 10.5px; color: var(--muted); }

  .row { display: flex; align-items: center; gap: 9px; padding: 6px; border-radius: 6px; }
  .row:hover { background: color-mix(in srgb, var(--text) 3.5%, transparent); }
  /* A 2px tick of the list's colour: the same vocabulary the grid's blocks
     use for the calendar they belong to. */
  .tick { width: 2px; height: 13px; flex: 0 0 2px; border-radius: 1px; }
  .title { appearance: none; -webkit-appearance: none; font: inherit; text-align: left;
           border: 0; background: none; color: var(--text); flex-grow: 1; min-width: 0;
           padding: 0; cursor: pointer; overflow: hidden; text-overflow: ellipsis;
           white-space: nowrap; }
  .title:disabled { cursor: default; }
  .due { font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums;
         white-space: nowrap; }
  .due.overdue { color: var(--error); }
  .del { font: inherit; color: var(--muted); background: none; border: 0; cursor: pointer;
         padding: 0 2px; visibility: hidden; }
  .row:hover .del { visibility: visible; }
  .del:hover { color: var(--text); }
  .row.done .title { color: var(--muted); text-decoration: line-through; }

  .editor { background: var(--surface); border-radius: 8px; padding: 9px;
            box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent);
            display: flex; flex-direction: column; gap: 8px; margin: 2px 0; }
  .etitle { font: inherit; font-weight: 500; color: var(--text); background: none; border: 0;
            border-bottom: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
            padding: 0 0 3px; }
  .quick { display: flex; flex-wrap: wrap; gap: 4px; }
  .quick button { appearance: none; -webkit-appearance: none; font: inherit; font-size: 11px;
                  padding: 3px 8px; border-radius: 5px; border: 0; cursor: pointer;
                  background: color-mix(in srgb, var(--text) 5%, transparent); color: var(--text); }
  .quick button.clear { background: none; color: var(--muted); }
  .when { display: flex; gap: 6px; }
  .when input { font: inherit; font-size: 11px; padding: 4px 7px; border-radius: 5px;
                border: 1px solid var(--hairline); background: var(--bg); color: var(--text);
                font-variant-numeric: tabular-nums; }
  .enotes { font: inherit; font-size: 11px; resize: vertical; padding: 6px 8px; border-radius: 5px;
            border: 1px solid var(--hairline); background: var(--bg); color: var(--text); }
  .eact { display: flex; justify-content: flex-end; gap: 6px; }
  .eact button { appearance: none; -webkit-appearance: none; font: inherit; font-size: 11.5px;
                 padding: 4px 11px; border-radius: 6px; cursor: pointer;
                 border: 1px solid var(--hairline); background: none; color: var(--text); }
  .eact .go { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .eact button:disabled, .quick button:disabled, .add input:disabled { opacity: .5; cursor: default; }

  .empty { color: var(--muted); padding: 10px 6px; line-height: 1.5; }
</style>
