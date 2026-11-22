// Code previews: find the embedded lines for a path:line, highlight them, show them in the panel.
function codePreview(text, at) {
  const m = text.match(PATH_RE);
  if (!m || !m[2]) return null;
  const key = `${at}:${m[1]}`;
  const from = Number(m[2]), to = Number(m[3] || m[2]);
  const file = codeFiles[key]?.find((window) => from >= window.start && to >= from && to < window.start + window.lines.length);
  return file ? { key: `${key}:${file.start}`, file, from, to, path: m[1] } : null;
}
const highlightedFiles = new Map();
function codePre({ key, file, from, to, path }) {
  const pre = el('pre', 'code');
  const ext = path.match(/\.([\w]+)$/)?.[1].toLowerCase();
  let highlighted = highlightedFiles.get(key);
  if (!highlightedFiles.has(key) && window.hljs && ext && hljs.getLanguage(ext)) {
    const open = [];
    highlighted = hljs.highlight(file.lines.join('\n'), { language: ext }).value.split('\n').map((line, i) => {
      const prefix = open.join('');
      // Balance each row without losing spans that continue across source lines.
      for (const tag of line.match(/<span\b[^>]*>|<\/span>/g) || []) {
        if (tag === '</span>') open.pop();
        else open.push(tag);
      }
      return prefix + line + (file.lines[i] ? '' : ' ') + '</span>'.repeat(open.length);
    });
    highlightedFiles.set(key, highlighted);
  }
  file.lines.forEach((line, i) => {
    const n = file.start + i;
    const row = el('span', `code-line${n >= from && n <= to ? ' cited' : ''}`);
    if (highlighted) row.innerHTML = highlighted[i];
    else row.textContent = line || ' ';
    row.prepend(el('span', 'line-number', n));
    pre.append(row);
  });
  return pre;
}
function showCode(text, preview, href, button, before) {
  // The same button again closes its preview.
  const open = button.getAttribute('aria-expanded') === 'true';
  closePanel?.();
  if (open) return;
  const located = flows.filter((f) => f.before === before).flatMap((f) => f.locate(preview));
  const parts = [];
  if (located.length) {
    const hits = el('div', 'flow-hits', 'In the flow');
    for (const { node, reveal } of located) {
      const b = el('button', 'btn');
      b.type = 'button';
      b.setAttribute('aria-label', `Show ${node.label} in the flow`);
      if (node.n) b.append(el('span', 'n', node.n));
      b.append(node.label);
      b.addEventListener('click', reveal);
      hits.append(b);
    }
    parts.push(hits);
  }
  const pre = codePre(preview);
  const close = showPanel({
    label: 'Code preview',
    title: [el('code', null, text)],
    path: text,
    href,
    wrap: true,
    body: [...parts, pre],
    onClose: () => {
      button.setAttribute('aria-expanded', 'false');
      for (const f of flows) f.locate(null);
      button.focus({ preventScroll: true });
    },
  });
  button.setAttribute('aria-expanded', 'true');
  close.focus({ preventScroll: true });
  scrollCode(pre);
}
function scrollCode(pre) {
  const first = pre.querySelector('.cited');
  if (first) pre.scrollTop += first.getBoundingClientRect().top - pre.getBoundingClientRect().top
    - 3 * parseFloat(getComputedStyle(pre).lineHeight);
}
// A preview in a box's file list opens at its default height. The bar under it sets the height, up to
// the panel's scroll area however long the list; double-click restores the default.
function filePreview(li, preview) {
  const min = 120;
  const pre = codePre(preview);
  const grip = el('div', 'preview-resize');
  grip.tabIndex = 0;
  grip.setAttribute('role', 'separator');
  grip.setAttribute('aria-orientation', 'horizontal');
  grip.setAttribute('aria-label', 'Resize preview height');
  grip.setAttribute('aria-valuemin', min);
  li.append(pre, grip);
  // On phones the body has no box and the sheet itself scrolls.
  const body = li.closest('.detail-body');
  const scroller = getComputedStyle(body).display === 'contents' ? sidePanel : body;
  // Grows the preview past the panel first, so a panel without a set height reaches its tallest.
  const tallest = () => {
    const height = pre.style.height;
    pre.style.height = '100vh';
    const style = getComputedStyle(scroller);
    const room = scroller.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    const max = Math.max(min, room - (li.offsetHeight - pre.offsetHeight));
    pre.style.height = height;
    return max;
  };
  const setHeight = (height, max = tallest()) => {
    if (height !== null) pre.style.height = `${Math.round(Math.min(Math.max(min, height), max))}px`;
    grip.setAttribute('aria-valuemax', max);
    grip.setAttribute('aria-valuenow', pre.offsetHeight);
  };
  let drag = null;
  grip.addEventListener('pointerdown', (ev) => {
    if (ev.button !== 0) return;
    ev.preventDefault();
    grip.focus({ preventScroll: true });
    grip.setPointerCapture(ev.pointerId);
    drag = { id: ev.pointerId, y: ev.clientY, height: pre.offsetHeight, max: tallest() };
  });
  grip.addEventListener('pointermove', (ev) => {
    if (drag?.id === ev.pointerId) setHeight(drag.height + ev.clientY - drag.y, drag.max);
  });
  grip.addEventListener('lostpointercapture', () => { drag = null; });
  grip.addEventListener('keydown', (ev) => {
    const step = { ArrowUp: -32, ArrowDown: 32 }[ev.key];
    if (!step) return;
    ev.preventDefault();
    setHeight(pre.offsetHeight + step);
  });
  grip.addEventListener('dblclick', () => {
    pre.style.height = '';
    setHeight(null);
  });
  setHeight(null);
  scrollCode(pre);
}
