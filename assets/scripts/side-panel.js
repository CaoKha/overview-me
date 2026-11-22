// The one side panel, for a code preview or a pinned box's files; closePanel closes whichever is open.
let closePanel = null;
document.addEventListener('keydown', (ev) => { if (ev.key === 'Escape') closePanel?.(); });
const sidePanel = el('div', 'detail');
sidePanel.hidden = true;
sidePanel.setAttribute('role', 'region');
document.body.append(sidePanel);
const panelGrips = panelResize(sidePanel);
// Opens the side panel with `title` and its tools in the head (copy when there is a path, wrap when
// it can show code, open when there is a link, close) and `body` below. `onClose` runs while the
// panel is still open. Returns the close button.
function showPanel({ label, title, path, href, wrap, live, body, onClose }) {
  closePanel?.();
  const head = el('div', 'detail-head');
  head.append(...title);
  const tools = el('div', 'tools');
  if (path) {
    const b = iconButton(COPY, 'Copy path');
    b.addEventListener('click', () => copy(path, b, CHECK));
    tools.append(b);
  }
  if (wrap) {
    const b = iconButton(WRAP, 'Wrap long lines');
    b.setAttribute('aria-pressed', String(document.documentElement.classList.contains('code-wrap')));
    b.addEventListener('click', () => {
      const on = document.documentElement.classList.toggle('code-wrap');
      b.setAttribute('aria-pressed', String(on));
      try { localStorage.setItem(codeWrapKey, on ? '1' : '0'); } catch {}
      sidePanel.querySelectorAll('pre.code').forEach(scrollCode);
    });
    tools.append(b);
  }
  if (href) tools.append(openLink(href));
  if (tools.children.length) head.append(tools);
  const close = iconButton(CLOSE, 'Close (Esc)');
  close.classList.add('close');
  close.addEventListener('click', () => closePanel());
  head.append(close);
  sidePanel.setAttribute('aria-label', label);
  sidePanel.setAttribute('aria-live', live ? 'polite' : 'off');
  sidePanel.replaceChildren(...panelGrips, head, ...body);
  sidePanel.hidden = false;
  closePanel = () => {
    closePanel = null;
    onClose();
    sidePanel.hidden = true;
    sidePanel.replaceChildren();
  };
  return close;
}
