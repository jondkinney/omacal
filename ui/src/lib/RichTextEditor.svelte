<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { normalizeDescriptionHref, sanitizeDescriptionHtml } from './sanitize';

  let { value = $bindable('') }: { value: string } = $props();

  let richtext = $state<HTMLDivElement>();
  let editor = $state<HTMLDivElement>();
  let linkInput = $state<HTMLInputElement>();
  let linking = $state(false);
  let linkDraft = $state('https://');
  let linkError = $state(false);
  let linkRange: Range | null = null;

  onMount(() => {
    if (!editor) return;
    const safe = sanitizeDescriptionHtml(value);
    editor.innerHTML = safe;
    // Once this is an HTML editor, saving the original hostile markup while
    // displaying a cleaned version would be a trap. The visible, sanitised
    // document is the value from the moment the editor opens.
    value = safe;
  });

  function sync() {
    if (editor) value = sanitizeDescriptionHtml(editor.innerHTML);
  }

  // `sync` keeps the bound value safe on every keystroke without moving the
  // caret. Once focus leaves the whole editor, it is safe to reflect the
  // auto-linked value back into the contenteditable as well. Moving from the
  // document to the link row is internal and must preserve its saved range.
  function settle(event: FocusEvent) {
    if (!editor) return;
    if (event.relatedTarget instanceof Node && richtext?.contains(event.relatedTarget)) return;
    const safe = sanitizeDescriptionHtml(editor.innerHTML);
    if (editor.innerHTML !== safe) editor.innerHTML = safe;
    value = safe;
  }

  function run(command: string, argument?: string) {
    editor?.focus();
    document.execCommand(command, false, argument);
    sync();
  }

  function heading() {
    const current = String(document.queryCommandValue('formatBlock')).toLowerCase();
    run('formatBlock', current === 'h3' ? 'p' : 'h3');
  }

  async function startLink() {
    const selection = window.getSelection();
    const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
    linkRange = range && editor?.contains(range.commonAncestorContainer) ? range.cloneRange() : null;
    const selected = selection?.toString().trim() ?? '';
    linkDraft = normalizeDescriptionHref(selected) ?? 'https://';
    linkError = false;
    linking = true;
    await tick();
    linkInput?.focus();
    linkInput?.select();
  }

  function cancelLink() {
    linking = false;
    linkError = false;
    linkRange = null;
    editor?.focus();
  }

  function applyLink() {
    const href = normalizeDescriptionHref(linkDraft);
    if (!href || !editor) {
      linkError = true;
      return;
    }

    editor.focus();
    const selection = window.getSelection();
    if (linkRange && selection) {
      selection.removeAllRanges();
      selection.addRange(linkRange);
    }

    const active = selection?.rangeCount ? selection.getRangeAt(0) : null;
    if (active && !active.collapsed && editor.contains(active.commonAncestorContainer)) {
      document.execCommand('createLink', false, href);
    } else {
      const anchor = document.createElement('a');
      anchor.href = href;
      anchor.textContent = href.replace(/^mailto:/i, '');
      const range = active && editor.contains(active.commonAncestorContainer)
        ? active
        : document.createRange();
      if (!active || !editor.contains(active.commonAncestorContainer)) range.selectNodeContents(editor);
      range.collapse(false);
      range.insertNode(anchor);
      range.setStartAfter(anchor);
      range.collapse(true);
      selection?.removeAllRanges();
      selection?.addRange(range);
    }

    linking = false;
    linkError = false;
    linkRange = null;
    sync();
  }

  function paste(event: ClipboardEvent) {
    event.preventDefault();
    insertTransferred(event.clipboardData);
  }

  function insertTransferred(data: DataTransfer | null) {
    const html = data?.getData('text/html') ?? '';
    if (html) document.execCommand('insertHTML', false, sanitizeDescriptionHtml(html));
    else document.execCommand('insertText', false, data?.getData('text/plain') ?? '');
    sync();
  }

  function drop(event: DragEvent) {
    // A drop is another HTML ingress path. Letting the browser handle it
    // would briefly insert arbitrary markup (including an <img onerror>)
    // before the input handler could clean the stored value.
    event.preventDefault();
    editor?.focus();
    const range = document.caretRangeFromPoint(event.clientX, event.clientY);
    const selection = window.getSelection();
    if (range && editor?.contains(range.commonAncestorContainer) && selection) {
      selection.removeAllRanges();
      selection.addRange(range);
    }
    insertTransferred(event.dataTransfer);
  }

  function formatShortcut(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
    const key = event.key.toLowerCase();
    const command = { b: 'bold', i: 'italic', u: 'underline' }[key];
    if (command) {
      event.preventDefault();
      run(command);
    } else if (key === 'k') {
      event.preventDefault();
      void startLink();
    }
  }
</script>

<div class="richtext" bind:this={richtext} data-testid="description-editor" onfocusout={settle}>
  <div class="toolbar" role="toolbar" aria-label="Description formatting">
    <button type="button" aria-label="Bold" aria-keyshortcuts="Control+B Meta+B" title="Bold (Ctrl+B)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('bold')}><b>B</b></button>
    <button type="button" aria-label="Italic" aria-keyshortcuts="Control+I Meta+I" title="Italic (Ctrl+I)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('italic')}><i>I</i></button>
    <button type="button" aria-label="Underline" aria-keyshortcuts="Control+U Meta+U" title="Underline (Ctrl+U)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('underline')}><u>U</u></button>
    <button type="button" aria-label="Heading" title="Heading"
      onmousedown={(e) => e.preventDefault()} onclick={heading}><b>H</b></button>
    <button type="button" class="linkbutton" aria-label="Link"
      aria-keyshortcuts="Control+K Meta+K" title="Link (Ctrl+K)"
      onmousedown={(e) => e.preventDefault()} onclick={startLink}>Link</button>
  </div>
  {#if linking}
    <div class="linkrow">
      <input
        bind:this={linkInput}
        bind:value={linkDraft}
        aria-label="Link URL"
        aria-invalid={linkError ? 'true' : undefined}
        placeholder="https://example.com"
        oninput={() => (linkError = false)}
        onkeydown={(e) => {
          if (e.key === 'Enter') { e.preventDefault(); applyLink(); }
          else if (e.key === 'Escape') { e.preventDefault(); cancelLink(); }
        }}
      />
      <button type="button" onclick={applyLink}>Apply</button>
      <button type="button" class="cancel" aria-label="Cancel link" onclick={cancelLink}>×</button>
    </div>
  {/if}
  <div
    class="editor"
    bind:this={editor}
    contenteditable="true"
    role="textbox"
    tabindex="0"
    aria-label="Description"
    aria-multiline="true"
    data-placeholder="Add notes or an agenda"
    oninput={sync}
    onpaste={paste}
    ondrop={drop}
    onkeydown={formatShortcut}
  ></div>
</div>

<style>
  .richtext {
    overflow: hidden;
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border: 1px solid var(--hairline);
    border-radius: 5px;
  }
  .richtext:focus-within { outline: 1px solid var(--accent); outline-offset: -1px; }

  .toolbar { display: flex; align-items: center; gap: 2px; padding: 3px 4px;
             border-bottom: 1px solid var(--hairline); }
  .toolbar button, .linkrow button {
    width: 24px; height: 22px; padding: 0; border: 0; border-radius: 4px;
    background: none; color: var(--muted); font: inherit; font-size: 11px; cursor: pointer;
  }
  .toolbar button:hover, .linkrow button:hover {
    color: var(--text); background: color-mix(in srgb, var(--text) 8%, transparent);
  }
  .toolbar .linkbutton { width: auto; padding: 0 5px; }

  .linkrow { display: flex; align-items: center; gap: 3px; padding: 4px;
             border-bottom: 1px solid var(--hairline); }
  .linkrow input { flex: 1; min-width: 0; width: auto; padding: 3px 5px;
                   border: 1px solid var(--hairline); border-radius: 4px;
                   background: color-mix(in srgb, var(--text) 4%, transparent);
                   color: var(--text); font: inherit; font-size: 11px; }
  .linkrow input:focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  .linkrow button:first-of-type { width: auto; padding: 0 7px; }
  .linkrow .cancel { font-size: 14px; }

  .editor { min-height: 84px; max-height: 190px; overflow-y: auto; padding: 7px;
            color: var(--text); font: inherit; font-size: 12px; line-height: 1.45;
            white-space: pre-wrap; overflow-wrap: anywhere; }
  .editor:focus { outline: none; }
  .editor:empty::before { content: attr(data-placeholder); color: var(--muted); opacity: .65; }
  :global(.editor p), :global(.editor div) { margin: 0 0 .55em; }
  :global(.editor h2), :global(.editor h3) { margin: .25em 0 .45em; font-size: 1.15em; }
  :global(.editor ul), :global(.editor ol) { margin: .3em 0; padding-left: 1.5em; }
  :global(.editor a) { color: var(--accent); }
</style>
