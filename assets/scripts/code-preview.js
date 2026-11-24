// Code previews: find the embedded lines for a path or path:line, highlight them, show them in the panel.
function codePreview(text, at) {
  const m = text.match(PATH_RE);
  if (!m) return null;
  const key = `${at}:${m[1]}`;
  // A file without lines previews its first embedded window, with no line cited.
  if (!m[2]) {
    const file = codeFiles[key]?.[0];
    return file ? { key: `${key}:${file.start}`, file, from: file.start, to: file.start + file.lines.length - 1,
      path: m[1], whole: !!file.whole, bare: true } : null;
  }
  const from = Number(m[2]), to = Number(m[3] || m[2]);
  const file = codeFiles[key]?.find((window) => from >= window.start && to >= from && to < window.start + window.lines.length);
  return file ? { key: `${key}:${file.start}`, file, from, to, path: m[1], whole: !!file.whole } : null;
}
const highlightedFiles = new Map();
function highlightedLines({ key, file, path }) {
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
  return highlighted;
}
function codeLine(preview, i, highlighted, classes, marker = '') {
  const row = el('span', `code-line${classes}`);
  if (highlighted) row.innerHTML = highlighted[i];
  else row.textContent = preview.file.lines[i] || ' ';
  row.prepend(el('span', 'line-number', `${preview.file.start + i}${marker ? ` ${marker}` : ''}`));
  return row;
}
// Stands for the lines between two shown parts of a file; `lines` is unknown when the parts overlap.
function codeGap(lines) {
  const row = el('span', 'code-line gap');
  row.append(el('span', 'line-number'), `⋯ ${lines > 0 ? `${lines} line${lines === 1 ? '' : 's'} ` : ''}not embedded ⋯`);
  return row;
}
// Previews of one file share a pre in line order: overlapping windows merge, gaps between them get a row.
function codePre(...previews) {
  const pre = el('pre', 'code');
  const rows = new Map();
  for (const p of previews) p.file.lines.forEach((_, i) => rows.has(p.file.start + i) || rows.set(p.file.start + i, [p, i]));
  let last = null;
  for (const n of [...rows.keys()].sort((a, b) => a - b)) {
    if (last !== null && n > last + 1) pre.append(codeGap(n - last - 1));
    const [preview, i] = rows.get(n);
    pre.append(codeLine(preview, i, highlightedLines(preview), previews.some((p) => !p.bare && n >= p.from && n <= p.to) ? ' cited' : ''));
    last = n;
  }
  return pre;
}
function lineDiff(base, head) {
  let start = 0, end = 0;
  while (start < base.length && start < head.length && base[start] === head[start]) start++;
  while (end < base.length - start && end < head.length - start
    && base[base.length - end - 1] === head[head.length - end - 1]) end++;
  const a = base.slice(start, base.length - end), b = head.slice(start, head.length - end);
  const width = b.length + 1;
  // ponytail: Bound the quadratic LCS table so large ranges keep the plain base preview.
  if ((a.length + 1) * width > 4_000_000) return null;
  const lcs = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i * width + j] = a[i] === b[j] ? 1 + lcs[(i + 1) * width + j + 1]
        : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
    }
  }
  const rows = base.slice(0, start).map((line, i) => ['', i]);
  let i = 0, j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      rows.push(['', start + i++]);
      j++;
    } else if (i < a.length && (j === b.length || lcs[(i + 1) * width + j] >= lcs[i * width + j + 1])) {
      rows.push(['removed', start + i++]);
    } else rows.push(['added', start + j++]);
  }
  for (let i = base.length - end; i < base.length; i++) rows.push(['', i]);
  return rows;
}
// `pairs` holds [base, head or null] previews of one file. Whole files diff once; otherwise each pair
// diffs its cited lines in base line order, and a pair without a diff shows its base lines.
function codeDiffPre(pairs) {
  const bases = pairs.map(([base]) => base);
  const whole = pairs.every(([base, head]) => base.whole && head?.whole);
  const lines = (p) => whole ? p.file.lines : p.file.lines.slice(p.from - p.file.start, p.to - p.file.start + 1);
  const parts = whole ? [pairs[0]] : [...pairs].sort(([a], [b]) => a.from - b.from);
  const diffs = parts.map(([base, head]) => head && lineDiff(lines(base), lines(head)));
  if (!diffs.some(Boolean)) return codePre(...bases);
  const pre = el('pre', 'code');
  let last = null;
  parts.forEach(([base, head], k) => {
    if (last !== null && base.from !== last + 1) pre.append(codeGap(base.from - last - 1));
    last = base.to;
    const highlighted = [highlightedLines(base), head && highlightedLines(head)];
    for (const [kind, i] of diffs[k] || lines(base).map((_, i) => ['', i])) {
      const added = kind === 'added';
      const preview = added ? head : base;
      const index = (whole ? 0 : preview.from - preview.file.start) + i;
      const n = preview.file.start + index;
      pre.append(codeLine(preview, index, highlighted[added ? 1 : 0],
        `${kind ? ` ${kind}` : ''}${!added && bases.some((p) => !p.bare && n >= p.from && n <= p.to) ? ' cited' : ''}`, kind ? added ? '+' : '−' : ''));
    }
  });
  return pre;
}

function showCode(text, preview, href, button, before) {
  // The same button again closes its preview.
  const open = button.getAttribute('aria-expanded') === 'true';
  const side = before ? 'before' : 'after';
  closePanel[side]?.();
  if (open) return;
  const sideFlows = flows.filter((f) => f.before === !!before);
  const located = sideFlows.flatMap((f) => f.locate(preview));
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
  // The change's other files in a folder tree, like a box's, hidden until "Show all changed files".
  const status = changeStatus(before);
  const others = [...status.keys()].filter((f) => f !== preview.path).sort();
  const body = el('div', 'detail-body');
  body.append(...parts, others.length ? fileSplit(new Map([[preview.path, 1]]), others, status, button, pre, () => {}) : pre);
  const close = showPanel({
    before,
    label: 'Code preview',
    title: [el('code', null, text)],
    path: text,
    href,
    tree: others.length > 0 && 'Show all changed files',
    wrap: true,
    body: [body],
    onClose: () => {
      button.setAttribute('aria-expanded', 'false');
      for (const f of sideFlows) f.locate(null);
      // Only the panel holding focus hands it back.
      if (panelHasFocus(side)) button.focus({ preventScroll: true });
    },
  });
  button.setAttribute('aria-expanded', 'true');
  close.focus({ preventScroll: true });
  scrollCode(pre);
}
function scrollCode(pre, first = pre.querySelector('.cited')) {
  if (first) pre.scrollTop += first.getBoundingClientRect().top - pre.getBoundingClientRect().top
    - 3 * parseFloat(getComputedStyle(pre).lineHeight);
}

// `git diff --name-status` letters, as words for the folder tree's change marks.
const CHANGE_WORDS = { A: 'added', M: 'modified', D: 'deleted', R: 'renamed', C: 'copied', T: 'type changed' };
// The change's files on one side, with their status: Before has no added file, After no deleted one.
function changeStatus(before) {
  const status = new Map();
  for (const [s, path, old] of cfg.changed || []) {
    if (before ? s === 'A' || s === 'C' : s === 'D') continue;
    const word = CHANGE_WORDS[s] || s;
    status.set(before && s === 'R' ? old : path, [s, old ? `${word} ${before ? `to ${path}` : `from ${old}`}` : word]);
  }
  return status;
}

// A panel's folder tree beside `pane`: its files (path to range count) to pick from, then the
// change's `others`, muted and hidden until "Show all changed files". `node` picks the forge side.
function fileSplit(counts, others, status, node, pane, show) {
  const buttons = [];
  // Folders hold their children in first-seen order; a file is keyed by its full path.
  const root = new Map();
  for (const f of [...counts.keys(), ...others]) {
    let dir = root;
    for (const part of f.split('/').slice(0, -1)) {
      if (!dir.has(`${part}/`)) dir.set(`${part}/`, new Map());
      dir = dir.get(`${part}/`);
    }
    dir.set(f, null);
  }
  const branch = (dir) => {
    const list = el('ul');
    for (let [name, sub] of dir) {
      const li = el('li');
      if (sub) {
        // A folder whose only child is a folder shows as one row, like `assets/scripts/`.
        while (sub.size === 1) {
          const [[child, grandchildren]] = sub;
          if (!grandchildren) break;
          name += child;
          sub = grandchildren;
        }
        const folder = el('details');
        // A folder with none of the panel's files is part of the hidden rest of the change.
        folder.open = cites(sub);
        if (!folder.open) li.classList.add('tree-other');
        folder.append(el('summary', null, name), branch(sub));
        li.append(folder);
      } else if (!counts.has(name)) {
        li.classList.add('tree-other');
        const href = forgeHref(name, node);
        const row = el(href ? 'a' : 'span', 'tree-file', name.split('/').pop());
        row.title = name;
        if (href) Object.assign(row, { href, target: '_blank', rel: 'noopener' });
        row.append(mark(name));
        li.append(row);
      } else li.append(fileButton(name));
      list.append(li);
    }
    return list;
  };
  // The tree and the flat list each have a button per file: select marks both, and opens the
  // tree's folders around it in case the reader closed one before picking it in the flat list.
  const select = (name) => {
    for (const [b, path] of buttons) {
      if (path !== name) {
        b.removeAttribute('aria-current');
        continue;
      }
      b.setAttribute('aria-current', 'true');
      for (let d = b.closest('details'); d; d = d.parentElement.closest('details')) d.open = true;
    }
  };
  const fileButton = (name) => {
    const count = counts.get(name);
    const b = el('button', 'tree-file');
    b.type = 'button';
    b.title = name;
    const changed = status.has(name) ? `, ${status.get(name)[1]}` : '';
    b.setAttribute('aria-label', `${name}, ${count} range${count === 1 ? '' : 's'}${changed}`);
    // A cited folder (`skills/`) sits inside its own folder row, so it has no name of its own.
    b.append(el('span', null, name.split('/').pop() || 'whole folder'));
    if (status.has(name)) b.append(mark(name));
    b.append(el('span', 'count', count));
    b.addEventListener('click', () => {
      select(name);
      show(name);
    });
    buttons.push([b, name]);
    return b;
  };
  // Without "Show all changed files", the panel's files sit under their full folder path, unindented.
  const flat = () => {
    // Root files first, so none reads as part of the folder above it.
    const dirs = new Map([['', []]]);
    for (const f of counts.keys()) {
      const dir = f.slice(0, f.lastIndexOf('/') + 1);
      if (!dirs.has(dir)) dirs.set(dir, []);
      dirs.get(dir).push(f);
    }
    const list = el('ul', 'tree-flat');
    for (const [dir, files] of dirs) {
      if (dir) list.append(el('li', 'tree-dir', dir));
      for (const f of files) {
        const li = el('li');
        li.append(fileButton(f));
        list.append(li);
      }
    }
    return list;
  };
  const cites = (dir) => [...dir].some(([name, sub]) => (sub ? cites(sub) : counts.has(name)));
  const mark = (name) => {
    const [s, words] = status.get(name);
    const m = el('span', 'change');
    m.title = words;
    // Screen readers hear the words, not the letter.
    const letter = el('span', null, s);
    letter.setAttribute('aria-hidden', 'true');
    m.append(letter, el('span', 'sr-only', `, ${words}`));
    return m;
  };
  const tree = el('nav', 'file-tree');
  tree.setAttribute('aria-label', 'Files');
  tree.append(branch(root), flat());
  const split = el('div', counts.size === 1 ? 'file-split single' : 'file-split');
  split.append(tree, treeResize(split), pane);
  select(buttons[0][1]);
  return split;
}
