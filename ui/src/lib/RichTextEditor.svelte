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
  // Chromium may place typing back inside the preceding formatted element
  // when a collapsed range sits in an empty sibling text node. A temporary
  // zero-width caret anchor makes the sibling non-empty. It never enters the
  // bound/saved HTML and is removed from the live DOM when focus leaves.
  const CARET_ANCHOR = '\u200b';
  const withoutCaretAnchors = (text: string) => text.split(CARET_ANCHOR).join('');

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
    if (editor) value = sanitizeDescriptionHtml(withoutCaretAnchors(editor.innerHTML));
  }

  function input() {
    if (!editor || !editor.textContent?.includes(CARET_ANCHOR)) {
      sync();
      return;
    }

    const selection = window.getSelection();
    const active = selection?.rangeCount ? selection.getRangeAt(0) : null;
    const startNode = active?.startContainer instanceof Text ? active.startContainer : null;
    const endNode = active?.endContainer instanceof Text ? active.endContainer : null;
    let startOffset = active?.startOffset ?? 0;
    let endOffset = active?.endOffset ?? 0;
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    let node: Node | null;
    while ((node = walker.nextNode())) nodes.push(node as Text);

    for (const textNode of nodes) {
      if (!textNode.data.includes(CARET_ANCHOR)) continue;
      if (textNode === startNode) {
        startOffset -= textNode.data.slice(0, startOffset).split(CARET_ANCHOR).length - 1;
      }
      if (textNode === endNode) {
        endOffset -= textNode.data.slice(0, endOffset).split(CARET_ANCHOR).length - 1;
      }
      textNode.data = withoutCaretAnchors(textNode.data);
    }

    if (active && selection && startNode && endNode) {
      active.setStart(startNode, Math.max(0, startOffset));
      active.setEnd(endNode, Math.max(0, endOffset));
      selection.removeAllRanges();
      selection.addRange(active);
    }
    sync();
  }

  // `sync` keeps the bound value safe on every keystroke without moving the
  // caret. Once focus leaves the whole editor, it is safe to reflect the
  // auto-linked value back into the contenteditable as well. Moving from the
  // document to the link row is internal and must preserve its saved range.
  function settle(event: FocusEvent) {
    if (!editor) return;
    if (event.relatedTarget instanceof Node && richtext?.contains(event.relatedTarget)) return;
    const safe = sanitizeDescriptionHtml(withoutCaretAnchors(editor.innerHTML));
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

  /** Turn the token immediately behind a collapsed caret into the same safe
   * HTML `sync` would save, before the browser inserts its delimiter. Because
   * the replacement has identical visible text and the selection is moved
   * after its final node, Space/Enter lands outside the new anchor instead of
   * extending it. A URL split across formatting nodes is left for blur/save —
   * there is no single text token to replace without rewriting the selection. */
  function autoLinkBeforeDelimiter(event: KeyboardEvent) {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key !== ' ' && event.key !== 'Enter') return;
    if (!editor) return;

    const selection = window.getSelection();
    const caret = selection?.rangeCount ? selection.getRangeAt(0) : null;
    if (!caret?.collapsed || !editor.contains(caret.commonAncestorContainer)) return;
    if (!(caret.startContainer instanceof Text)) return;
    const textNode = caret.startContainer;
    if (textNode.parentElement?.closest('a')) return;

    const before = textNode.data.slice(0, caret.startOffset);
    const token = before.match(/\S+$/)?.[0];
    if (!token) return;

    // Start from textContent so characters such as `<` can never become HTML
    // during detection. The sanitizer performs both conservative recognition
    // and the final protocol allowlist.
    const plain = document.createElement('span');
    plain.textContent = token;
    const linked = sanitizeDescriptionHtml(plain.innerHTML);
    const template = document.createElement('template');
    template.innerHTML = linked;
    if (!template.content.querySelector('a')) return;

    const replacement = document.createRange();
    replacement.setStart(textNode, caret.startOffset - token.length);
    replacement.setEnd(textNode, caret.startOffset);
    replacement.deleteContents();
    const last = template.content.lastChild;
    if (!last) return;
    replacement.insertNode(template.content);

    const after = document.createRange();
    after.setStartAfter(last);
    after.collapse(true);
    selection?.removeAllRanges();
    selection?.addRange(after);
    sync();
  }

  /** Markdown's line-opening shortcuts are useful here as gestures, not as a
   * storage format. As soon as the marker's trailing space is pressed, remove
   * the marker and create the same HTML the toolbar does.
   * Requiring the marker to be the block's entire contents keeps prose such
   * as "budget - " from unexpectedly turning into a list. */
  function markdownBlockShortcut(event: KeyboardEvent): boolean {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.key !== ' ') {
      return false;
    }
    if (!editor) return false;

    const selection = window.getSelection();
    const caret = selection?.rangeCount ? selection.getRangeAt(0) : null;
    if (!caret?.collapsed || !editor.contains(caret.commonAncestorContainer)) return false;

    const parent = caret.startContainer instanceof Element
      ? caret.startContainer
      : caret.startContainer.parentElement;
    let block = parent?.closest('li, p, div, h2, h3') as HTMLElement | null;
    if (!block || !editor.contains(block)) block = editor;
    // A marker at the beginning of an existing list item is content, not a
    // request to toggle the surrounding list off.
    if (block.closest('li')) return false;

    const before = document.createRange();
    before.selectNodeContents(block);
    try {
      before.setEnd(caret.startContainer, caret.startOffset);
    } catch {
      return false;
    }

    const marker = before.toString();
    if ((block.textContent ?? '') !== marker) return false;
    const tag = marker === '-' || marker === '*'
      ? 'ul'
      : marker === '1.'
        ? 'ol'
        : marker === '#'
          ? 'h3'
          : null;
    if (!tag) return false;

    event.preventDefault();
    const replacement = document.createElement(tag);
    const caretBlock = tag === 'ul' || tag === 'ol'
      ? replacement.appendChild(document.createElement('li'))
      : replacement;
    caretBlock.append(document.createElement('br'));
    if (block === editor) editor.replaceChildren(replacement);
    else block.replaceWith(replacement);

    // Do not leave this to execCommand's block-affinity rules: WebKit can put
    // a collapsed selection after the new list, visually on the line below
    // its empty bullet. Offset zero is immediately inside the <li>, before
    // its placeholder break, so the first typed character has no leading gap.
    const inside = document.createRange();
    inside.setStart(caretBlock, 0);
    inside.collapse(true);
    selection?.removeAllRanges();
    selection?.addRange(inside);
    sync();
    return true;
  }

  /** Replace a just-completed inline Markdown-style run with safe rich HTML.
   * The closing character is intercepted before it enters the document, and
   * a temporary caret anchor after the new element gives typing an
   * unformatted place to continue. Single stars intentionally mean bold here,
   * matching the compact invocation shown to users; conventional **bold**
   * works too.
   * Markdown has no native underline marker, so this editor uses the common
   * extension spelling ++underline++. */
  function markdownInlineShortcut(event: KeyboardEvent): boolean {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return false;
    if (!editor || !['*', '_', '+'].includes(event.key)) return false;

    const selection = window.getSelection();
    const caret = selection?.rangeCount ? selection.getRangeAt(0) : null;
    if (!caret?.collapsed || !editor.contains(caret.commonAncestorContainer)) return false;
    if (!(caret.startContainer instanceof Text)) return false;

    const textNode = caret.startContainer;
    const before = textNode.data.slice(0, caret.startOffset);
    const rules: Array<{
      key: string;
      pattern: RegExp;
      openLength: number;
      partialCloseLength: number;
      tag: 'strong' | 'em' | 'u';
    }> = [
      { key: '*', pattern: /(?:^|[\s([{])\*\*([^*\n]+)\*$/, openLength: 2, partialCloseLength: 1, tag: 'strong' },
      { key: '*', pattern: /(?:^|[\s([{])\*([^*\n]+)$/, openLength: 1, partialCloseLength: 0, tag: 'strong' },
      { key: '_', pattern: /(?:^|[\s([{])_([^_\n]+)$/, openLength: 1, partialCloseLength: 0, tag: 'em' },
      { key: '+', pattern: /(?:^|[\s([{])\+\+([^+\n]+)\+$/, openLength: 2, partialCloseLength: 1, tag: 'u' },
    ];

    for (const rule of rules) {
      if (event.key !== rule.key) continue;
      const match = before.match(rule.pattern);
      const content = match?.[1];
      if (!content || content.trim() !== content) continue;

      const markerStart = caret.startOffset
        - rule.openLength
        - content.length
        - rule.partialCloseLength;
      if (markerStart < 0) continue;

      event.preventDefault();
      const prefix = withoutCaretAnchors(textNode.data.slice(0, markerStart));
      const suffix = withoutCaretAnchors(textNode.data.slice(caret.startOffset));
      const formatted = document.createElement(rule.tag);
      formatted.textContent = content;
      const tail = document.createElement('span');
      const tailText = document.createTextNode(CARET_ANCHOR + suffix);
      tail.append(tailText);
      const nodes: Node[] = [];
      if (prefix) nodes.push(document.createTextNode(prefix));
      nodes.push(formatted, tail);
      textNode.replaceWith(...nodes);

      const after = document.createRange();
      after.setStart(tailText, 1);
      after.collapse(true);
      selection?.removeAllRanges();
      selection?.addRange(after);
      sync();
      return true;
    }
    return false;
  }

  function editorKeydown(event: KeyboardEvent) {
    if (markdownBlockShortcut(event) || markdownInlineShortcut(event)) return;
    autoLinkBeforeDelimiter(event);
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
    <button type="button" aria-label="Bold" aria-keyshortcuts="Control+B Meta+B" title="Bold (Ctrl+B or *text*)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('bold')}><b>B</b></button>
    <button type="button" aria-label="Italic" aria-keyshortcuts="Control+I Meta+I" title="Italic (Ctrl+I or _text_)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('italic')}><i>I</i></button>
    <button type="button" aria-label="Underline" aria-keyshortcuts="Control+U Meta+U" title="Underline (Ctrl+U or ++text++)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('underline')}><u>U</u></button>
    <button type="button" aria-label="Heading" title="Heading (# then Space)"
      onmousedown={(e) => e.preventDefault()} onclick={heading}><b>H</b></button>
    <button type="button" class="listbutton" aria-label="Bulleted list" title="Bulleted list (- or * then Space)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('insertUnorderedList')}>•</button>
    <button type="button" class="listbutton" aria-label="Numbered list" title="Numbered list (1. then Space)"
      onmousedown={(e) => e.preventDefault()} onclick={() => run('insertOrderedList')}>1.</button>
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
    oninput={input}
    onpaste={paste}
    ondrop={drop}
    onkeydown={editorKeydown}
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
  .toolbar .listbutton { width: 27px; }

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
