// One movable code panel per side.
const closePanel = { before: null, after: null };
const sidePanels = Object.fromEntries(['before', 'after'].map((side) => {
  const panel = el('div', `detail ${side}`);
  panel.hidden = true;
  panel.setAttribute('role', 'region');
  document.body.append(panel);
  panel.append(...panelResize(panel));
  const front = () => {
    for (const p of Object.values(sidePanels)) p.style.zIndex = p === panel ? '10' : '9';
  };
  panel.addEventListener('pointerdown', front);
  panel.addEventListener('focusin', front);
  return [side, panel];
}));
const panelHasFocus = (side) => sidePanels[side].contains(document.activeElement);
document.addEventListener('keydown', (ev) => {
  if (ev.key !== 'Escape') return;
  const side = Object.keys(sidePanels).find(panelHasFocus);
  if (side) closePanel[side]?.();
  else { closePanel.before?.(); closePanel.after?.(); }
});
// A phone has room for one sheet: shrinking the window closes the panel behind.
addEventListener('resize', () => {
  if (innerWidth > 640 || !closePanel.before || !closePanel.after) return;
  closePanel[sidePanels.before.style.zIndex === '10' ? 'after' : 'before']();
});
// Opens the side panel with `title` and its tools in the head (copy when there is a path, the
// folder tree toggle labelled `tree` when the tree can show more, wrap when it can show code, open
// when there is a link, close) and `body` below. `onClose` runs while the panel is still open. Returns the close
// button.
function showPanel({ before = false, label, title, path, href, tree, wrap, live, body, onClose }) {
  const side = before ? 'before' : 'after';
  closePanel[side]?.();
  const other = before ? 'after' : 'before';
  if (innerWidth <= 640) closePanel[other]?.();
  const panel = sidePanels[side];
  panel.setAttribute('aria-label', `${before ? 'Before' : 'After'}: ${label}`);
  panel.setAttribute('aria-live', live ? 'polite' : 'off');
  const head = el('div', 'detail-head');
  head.append(el('span', 'tag', before ? 'Before' : 'After'), ...title);
  const tools = el('div', 'tools');
  if (path) {
    const b = iconButton(COPY, 'Copy path');
    b.addEventListener('click', () => copy(path, b, CHECK));
    tools.append(b);
  }
  if (tree) {
    const b = iconButton(TREE, tree);
    b.classList.add('tree-toggle');
    b.setAttribute('aria-pressed', String(document.documentElement.classList.contains('tree-all')));
    b.addEventListener('click', () => {
      const on = document.documentElement.classList.toggle('tree-all');
      document.querySelectorAll('.detail .tree-toggle').forEach((b) => b.setAttribute('aria-pressed', String(on)));
      try { localStorage.setItem(treeAllKey, on ? '1' : '0'); } catch {}
    });
    tools.append(b);
  }
  if (wrap) {
    const b = iconButton(WRAP, 'Wrap long lines');
    b.classList.add('wrap-toggle');
    b.setAttribute('aria-pressed', String(document.documentElement.classList.contains('code-wrap')));
    b.addEventListener('click', () => {
      const on = document.documentElement.classList.toggle('code-wrap');
      document.querySelectorAll('.detail .wrap-toggle').forEach((b) => b.setAttribute('aria-pressed', String(on)));
      try { localStorage.setItem(codeWrapKey, on ? '1' : '0'); } catch {}
      document.querySelectorAll('.detail pre.code').forEach(scrollCode);
    });
    tools.append(b);
  }
  if (href) tools.append(openLink(href));
  if (tools.children.length) head.append(tools);
  const close = iconButton(CLOSE, 'Close (Esc)');
  close.classList.add('close');
  close.addEventListener('click', () => closePanel[side]?.());
  head.append(close);
  panel.append(head, ...body);
  panel.hidden = false;
  panel.style.zIndex = '10';
  sidePanels[other].style.zIndex = '9';
  closePanel[side] = () => {
    closePanel[side] = null;
    onClose();
    panel.hidden = true;
    head.remove();
    for (const part of body) part.remove();
  };
  return close;
}
