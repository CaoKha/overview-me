// ```flow blocks: {"nodes": {id: {label, n?, kind?, note?, paths?}}, "tiers": [[{name, ids, note?, flex?}]],
// "edges": [[from, to, label?, style?, at?]]}. kind: existing | added | removed, plus store; style:
// dashed | added | removed; at: the references behind the arrow, as path:line.
const NS = 'http://www.w3.org/2000/svg';
let uid = 0;
const nodeKind = (n) => ['removed', 'added', 'existing'].find((k) => (n.kind || '').split(/\s+/).includes(k)) || 'changed';
// One { before, spec, locate } per flow diagram; `before` when it sits in a Before panel.
const flows = [];
// How well a box path holds path:from-to: 3 its lines, 2 its file, 1 its folder, 0 not at all.
function pathScore(boxPath, path, from, to) {
  if (boxPath.endsWith('/')) return path.startsWith(boxPath) ? 1 : 0;
  const m = boxPath.match(PATH_RE);
  if (!m || m[1] !== path) return 0;
  return m[2] && Number(m[2]) <= to && from <= Number(m[3] || m[2]) ? 3 : 2;
}

function legend(spec, before) {
  const kinds = new Set(Object.values(spec.nodes).map(nodeKind));
  const box = el('div', 'legend');
  if (['existing', 'added', 'removed'].some((k) => kinds.has(k))) {
    for (const [k, text] of [['changed', 'changed by this branch'], ['added', 'new'], ['removed', 'removed'], ['existing', 'unchanged']]) {
      if (!kinds.has(k)) continue;
      const s = el('span');
      s.append(el('i', k), text);
      box.append(s);
    }
  }
  const flagged = !before && Object.values(spec.nodes).some((n) => n.n && findingsByNode.has(n.n));
  box.append(el('span', null, flagged
    ? 'Hover a box to trace its links; click it for its findings and files. A badge counts review findings, red for critical and high.'
    : 'Hover a box to trace its links; click it for its files.'));
  return box;
}

function drawFlow(arch, spec, before) {
  const side = before ? 'before' : 'after';
  uid += 1;
  const id = uid;
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('class', 'edges');
  svg.setAttribute('aria-hidden', 'true');
  const marker = (name, color) => `<marker id="${name}${id}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" style="fill: var(${color})"></path></marker>`;
  svg.innerHTML = `<defs>${marker('a', '--edge')}${marker('h', '--hl')}${marker('g', '--ok')}${marker('r', '--bad')}</defs>`;
  arch.append(svg);

  const boxes = {};
  for (const tier of spec.tiers) {
    const t = el('div', 'tier');
    for (const lane of tier) {
      const l = el('div', 'lane');
      if (lane.flex) l.style.flex = lane.flex;
      l.append(el('span', 'lane-name', lane.name));
      const bx = el('div', 'lane-boxes');
      for (const nid of lane.ids) {
        const n = spec.nodes[nid];
        if (!n || boxes[nid]) continue;
        const b = el('div', `box ${n.kind || ''}`.trim());
        b.tabIndex = 0;
        if (n.n) b.append(el('span', 'n', n.n));
        b.append(el('span', 'label', n.label));
        const found = boxFindings(n, arch);
        if (found.length) {
          b.append(findingsBadge(found));
          b.dataset.findings = found.length;
        }
        bx.append(b);
        boxes[nid] = b;
      }
      l.append(bx);
      if (lane.note) l.append(el('span', 'lane-note', lane.note));
      t.append(l);
    }
    arch.append(t);
  }

  const edges = spec.edges.filter((e) => boxes[e[0]] && boxes[e[1]] && e[0] !== e[1]);
  const restMarker = (e) => (e[3] === 'added' ? 'g' : e[3] === 'removed' ? 'r' : 'a');
  const sites = (e) => (Array.isArray(e[4]) ? e[4].filter((a) => typeof a === 'string') : []);
  const arrowName = (e) => `${spec.nodes[e[0]].label} → ${spec.nodes[e[1]].label}`;
  const siteButton = (text, site) => {
    const b = el('button', 'site', text);
    b.type = 'button';
    b.setAttribute('aria-label', `Show ${site}`);
    b.setAttribute('aria-expanded', 'false');
    return b;
  };
  const paths = [];
  const labels = [];
  const hits = [];
  const hitSvg = document.createElementNS(NS, 'svg');
  hitSvg.setAttribute('class', 'edge-hits');
  hitSvg.setAttribute('aria-hidden', 'true');
  edges.forEach((e, i) => {
    const p = document.createElementNS(NS, 'path');
    p.setAttribute('class', `line ${e[3] || ''}`.trim());
    p.setAttribute('marker-end', `url(#${restMarker(e)}${id})`);
    svg.append(p);
    paths.push(p);
    if (e[2]) {
      const lb = el('span', `edge-label ${e[3] || ''}`.trim(), e[2]);
      arch.append(lb);
      labels[i] = lb;
    }
    if (sites(e).length) {
      const h = document.createElementNS(NS, 'path');
      const t = document.createElementNS(NS, 'title');
      t.textContent = `${arrowName(e)}\n${sites(e).join('\n')}`;
      h.append(t);
      h.addEventListener('mouseenter', () => { if (!pinned) focusOn(e[0], i); });
      h.addEventListener('mouseleave', () => { if (!pinned) clear(); });
      hitSvg.append(h);
      hits[i] = h;
    }
  });
  if (hits.length) arch.append(hitSvg);

  function draw() {
    const base = arch.getBoundingClientRect();
    svg.setAttribute('width', base.width);
    svg.setAttribute('height', base.height);
    const R = {};
    for (const k of Object.keys(boxes)) {
      const r = boxes[k].getBoundingClientRect();
      R[k] = { x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height };
    }
    // Each edge leaves and enters a box side; edges sharing a side spread along it.
    const ports = {};
    edges.forEach((e, i) => {
      const a = R[e[0]], b = R[e[1]];
      const acy = a.y + a.h / 2, bcy = b.y + b.h / 2;
      let sa, sb;
      if (Math.abs(acy - bcy) < 8) [sa, sb] = a.x < b.x ? ['right', 'left'] : ['left', 'right'];
      else [sa, sb] = bcy > acy ? ['bottom', 'top'] : ['top', 'bottom'];
      for (const [nid, side, other, self] of [[e[0], sa, b, 0], [e[1], sb, a, 1]]) {
        (ports[`${nid}:${side}`] ||= []).push({ i, other, self });
      }
    });
    const pos = {};
    for (const key of Object.keys(ports)) {
      const [nid, side] = key.split(':');
      const r = R[nid];
      const list = ports[key];
      const horiz = side === 'top' || side === 'bottom';
      list.sort((p, q) => (horiz ? (p.other.x + p.other.w / 2) - (q.other.x + q.other.w / 2) : p.other.y - q.other.y));
      list.forEach((p, k) => {
        const f = (k + 1) / (list.length + 1);
        const x = side === 'left' ? r.x : side === 'right' ? r.x + r.w : r.x + r.w * f;
        const y = side === 'top' ? r.y : side === 'bottom' ? r.y + r.h : r.y + r.h * f;
        pos[`${p.i}:${p.self}`] = { x, y, side };
      });
    }
    edges.forEach((e, i) => {
      const s = pos[`${i}:0`], t = pos[`${i}:1`];
      let ex = t.x, ey = t.y;
      if (t.side === 'top') ey -= 4; else if (t.side === 'bottom') ey += 4; else if (t.side === 'left') ex -= 4; else ex += 4;
      let d;
      if (s.side === 'top' || s.side === 'bottom') {
        const dy = Math.max(30, Math.abs(ey - s.y) / 2), sg = s.side === 'bottom' ? 1 : -1;
        d = `M${s.x},${s.y} C${s.x},${s.y + sg * dy} ${ex},${ey - sg * dy} ${ex},${ey}`;
      } else {
        const dx = Math.max(30, Math.abs(ex - s.x) / 2), sx = s.side === 'right' ? 1 : -1;
        d = `M${s.x},${s.y} C${s.x + sx * dx},${s.y} ${ex - sx * dx},${ey} ${ex},${ey}`;
      }
      paths[i].setAttribute('d', d);
      hits[i]?.setAttribute('d', d);
      if (labels[i]) {
        const m = paths[i].getPointAtLength(paths[i].getTotalLength() / 2);
        // Centred on the edge, but shifted inward when that would cut it off at the diagram's side.
        const half = labels[i].offsetWidth / 2, pad = 4;
        labels[i].style.left = `${Math.min(Math.max(m.x, half + pad), base.width - half - pad)}px`;
        labels[i].style.top = `${s.side === 'left' || s.side === 'right' ? m.y - 13 : m.y}px`;
      }
    });
  }

  // A box traces all its arrows; with `only`, just that arrow.
  function focusOn(nid, only) {
    arch.classList.add('focus');
    boxes[nid].classList.add('near');
    edges.forEach((e, i) => {
      const on = only === undefined ? e[0] === nid || e[1] === nid : i === only;
      paths[i].classList.toggle('on', on);
      paths[i].setAttribute('marker-end', `url(#${on ? 'h' : restMarker(e)}${id})`);
      if (labels[i]) labels[i].classList.toggle('on', on);
      if (on) { boxes[e[0]].classList.add('near'); boxes[e[1]].classList.add('near'); }
    });
  }
  function clear() {
    arch.classList.remove('focus');
    for (const b of Object.values(boxes)) b.classList.remove('near');
    edges.forEach((e, i) => {
      paths[i].classList.remove('on');
      paths[i].setAttribute('marker-end', `url(#${restMarker(e)}${id})`);
      if (labels[i]) labels[i].classList.remove('on');
    });
  }
  // Click (or Enter) pins a box: its links stay traced and the panel lists its files.
  let pinned = null;
  function showDetail(k) {
    const n = spec.nodes[k];
    const title = [];
    if (n.n) title.push(el('span', 'n', n.n));
    title.push(el('span', null, n.label));
    const kind = nodeKind(n);
    const after = before && kind === 'changed' ? flows.find((f) => !f.before && f.spec.nodes[k])?.spec.nodes[k] : null;
    const headPaths = Array.isArray(after?.paths) ? after.paths : [];
    title.push(el('span', 'detail-kind', { changed: n.n ? 'changed' : '', added: 'new', removed: 'removed', existing: 'unchanged' }[kind]));
    const body = el('div', 'detail-body');
    let previews = false;
    if (n.note) body.append(el('p', 'detail-note', n.note));
    const found = boxFindings(n, arch);
    if (found.length) body.append(findingsList(found, arch));
    const paths = Array.isArray(n.paths) ? n.paths : [];
    const pane = el('div', 'file-pane');
    pane.setAttribute('aria-live', 'off');
    const at = previewRev(before);
    const status = changeStatus(before);
    let others = [];
    let many = false;
    if (!paths.length) body.append(el('p', 'none', 'No file given for this box.'));
    else {
      const file = (q) => q.match(PATH_RE)?.[1] ?? q;
      // Each file's indexes into `paths`, in first-seen order.
      const files = new Map();
      paths.forEach((p, i) => files.set(file(p), [...files.get(file(p)) || [], i]));
      many = files.size > 1;
      const pair = (i) => {
        const preview = codePreview(paths[i], at);
        if (!preview) return null;
        const f = file(paths[i]);
        // The nth range of a file at base pairs with the nth range of that file at head.
        const nth = paths.slice(0, i).filter((q) => file(q) === f).length;
        const same = headPaths.filter((q) => file(q) === f);
        // Same index only for a file missing at head, such as a rename.
        const headPath = same.length ? same[nth] : headPaths[i];
        return [preview, headPath ? codePreview(headPath, previewRev(false)) : null];
      };
      const pairs = (f) => files.get(f).map(pair).filter(Boolean);
      previews = [...files.keys()].some((f) => pairs(f).length);
      const show = (f) => {
        const own = files.get(f).map((i) => paths[i]);
        const bar = el('div', 'file-bar');
        bar.append(el('code', null, f));
        const rangeList = el('span', 'ranges');
        if (own.some((p) => p.length > f.length)) bar.append(rangeList);
        const b = iconButton(COPY, 'Copy path');
        b.addEventListener('click', () => copy(f, b, CHECK));
        bar.append(b);
        const href = forgeHref(own[0], arch);
        if (href) bar.append(openLink(href));
        const render = (shown) => {
          const code = !shown.length ? el('p', 'none', 'No preview')
            : shown.some(([, head]) => head) ? codeDiffPre(shown) : codePre(...shown.map(([base]) => base));
          // A removed or added box has no other side to diff, so its cited lines take the diff colors.
          if (kind === 'removed' || kind === 'added') {
            // A file without lines has no cited rows: all of it was added or removed.
            const rows = shown.every(([base]) => base.bare) ? '.code-line:not(.gap)' : '.cited';
            for (const row of code.querySelectorAll(rows)) {
              row.classList.add(kind);
              row.querySelector('.line-number').append(` ${kind === 'added' ? '+' : '−'}`);
            }
          }
          return code;
        };
        const code = render(pairs(f));
        // A range shows only its lines; clicking it again closes it and brings back every range.
        // Another range of the file replaces it, like the lines under an arrow's reference.
        for (const i of files.get(f)) {
          const range = paths[i].slice(f.length + 1);
          if (!range) continue;
          if (rangeList.childNodes.length) rangeList.append(', ');
          const p = pair(i);
          if (!p) {
            rangeList.append(range);
            continue;
          }
          const b = siteButton(range, paths[i]);
          b.addEventListener('click', () => {
            const open = b.getAttribute('aria-expanded') === 'true';
            for (const other of rangeList.querySelectorAll('.site')) other.setAttribute('aria-expanded', 'false');
            const shown = open ? code : render([p]);
            pane.replaceChildren(bar, shown);
            if (!open) b.setAttribute('aria-expanded', 'true');
            scrollCode(shown);
          });
          rangeList.append(b);
        }
        pane.replaceChildren(bar, code);
        if (code.isConnected) scrollCode(code);
      };
      // The change's other files join the tree after the box's own, hidden until the reader asks.
      const folders = [...files.keys()].filter((f) => f.endsWith('/'));
      others = [...status.keys()].filter((f) => !files.has(f) && !folders.some((d) => f.startsWith(d))).sort();
      if (files.size === 1 && !others.length) body.append(pane);
      else {
        body.append(fileSplit(new Map([...files].map(([f, i]) => [f, i.length])), others, status, arch, pane, show));
      }
      show(files.keys().next().value);
    }
    // The references behind this box's arrows, for readers who cannot hover.
    const linked = edges.filter((e) => (e[0] === k || e[1] === k) && sites(e).length);
    if (linked.length) {
      body.append(el('p', 'detail-note', 'Arrows come from these references:'));
      const list = el('ul', 'paths');
      for (const e of linked) {
        const li = el('li');
        li.append(el('span', null, arrowName(e)));
        // Each file once, with its lines beside it.
        const files = new Map();
        for (const s of sites(e)) {
          const [, path, from, to] = s.match(PATH_RE) || [, s];
          if (!files.has(path)) files.set(path, []);
          if (from) files.get(path).push(to ? `${from}-${to}` : from);
        }
        for (const [path, lines] of files) {
          const ranges = el('span', 'ranges');
          lines.forEach((line, i) => {
            if (i) ranges.append(', ');
            const site = `${path}:${line}`;
            const preview = codePreview(site, at);
            if (!preview) return ranges.append(line);
            // A line opens its call under the reference, so the reader can check the arrow without
            // losing the file above. Clicking it again closes it; another line of the row replaces it.
            const b = siteButton(line, site);
            b.addEventListener('click', () => {
              const open = li.querySelector(':scope > pre.code');
              const same = open?.dataset.site === site;
              open?.remove();
              for (const other of li.querySelectorAll('.site')) other.setAttribute('aria-expanded', 'false');
              if (same) return;
              const code = codePre(preview);
              code.dataset.site = site;
              li.append(code);
              b.setAttribute('aria-expanded', 'true');
              scrollCode(code);
              code.scrollIntoView({ block: 'nearest' });
            });
            ranges.append(b);
          });
          li.append(el('code', null, path), ranges);
        }
        list.append(li);
      }
      body.append(list);
    }
    const close = showPanel({ before, label: `${n.label} files`, title, tree: others.length ? 'Show all changed files' : many && 'Show folder tree', wrap: previews || found.some((f) => f.start), live: true, body: [body], onClose: unpinned });
    const pre = pane.querySelector('pre.code');
    if (pre) scrollCode(pre);
    return close;
  }
  function unpinned() {
    const box = boxes[pinned];
    const back = panelHasFocus(side) ? box.opener || box : null;
    box.opener = null;
    box.classList.remove('pinned');
    pinned = null;
    clear();
    back?.focus({ preventScroll: true });
  }
  // The card sits at the end of body, so a keyboard pin moves focus into it and closing it from
  // inside returns focus to what opened it (the box, or a node id chip in the text), where it is.
  function pin(k, fromKey) {
    const close = showDetail(k);
    pinned = k;
    clear();
    focusOn(k);
    boxes[k].classList.add('pinned');
    if (fromKey) close.focus({ preventScroll: true });
  }
  function unpin() {
    if (pinned) closePanel[side]?.();
  }
  for (const [k, b] of Object.entries(boxes)) {
    b.setAttribute('role', 'button');
    const count = Number(b.dataset.findings || 0);
    const flagged = count ? ` and ${count} finding${count === 1 ? '' : 's'}` : '';
    b.setAttribute('aria-label', `${spec.nodes[k].n ? `${spec.nodes[k].n} ` : ''}${spec.nodes[k].label}: show files${flagged}`);
    b.addEventListener('mouseenter', () => { if (!pinned) focusOn(k); });
    b.addEventListener('mouseleave', () => { if (!pinned) clear(); });
    b.addEventListener('focus', () => { if (!pinned) focusOn(k); });
    b.addEventListener('blur', () => { if (!pinned) clear(); });
    b.addEventListener('click', (ev) => { ev.stopPropagation(); if (pinned === k) unpin(); else pin(k); });
    b.addEventListener('keydown', (ev) => {
      if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); if (pinned === k) unpin(); else pin(k, true); }
    });
  }
  arch.addEventListener('click', unpin);
  draw();
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(draw);
  if (window.ResizeObserver) new ResizeObserver(draw).observe(arch);
  else window.addEventListener('resize', draw);

  // Highlights the boxes that hold a previewed path:line (only the best match: lines, then file,
  // then folder) and returns them with a way to scroll to each; null clears the highlight.
  return function locate(preview) {
    for (const b of Object.values(boxes)) b.classList.remove('located');
    if (!preview) return [];
    const { path, from, to } = preview;
    let best = 0;
    let keys = [];
    for (const k of Object.keys(boxes)) {
      const paths = Array.isArray(spec.nodes[k].paths) ? spec.nodes[k].paths : [];
      const score = Math.max(0, ...paths.map((p) => pathScore(p, path, from, to)));
      if (!score || score < best) continue;
      if (score > best) [best, keys] = [score, []];
      keys.push(k);
    }
    return keys.map((k) => {
      boxes[k].classList.add('located');
      const reveal = () => {
        const smooth = !matchMedia('(prefers-reduced-motion: reduce)').matches;
        // On phones the preview is a bottom sheet, so the box goes to the top of the screen.
        boxes[k].scrollIntoView({ block: innerWidth <= 640 ? 'start' : 'center', behavior: smooth ? 'smooth' : 'auto' });
        boxes[k].focus({ preventScroll: true });
      };
      return { node: spec.nodes[k], reveal };
    });
  };
}

for (const code of out.querySelectorAll('pre > code.language-flow')) {
  const pre = code.parentElement;
  let spec;
  try {
    spec = JSON.parse(code.textContent);
    if (!spec || typeof spec.nodes !== 'object' || !Array.isArray(spec.tiers)) throw new Error('needs "nodes" and "tiers"');
    spec.edges ||= [];
  } catch (err) {
    pre.before(el('p', 'flow-error', `Flow diagram not drawn: ${err.message}`));
    continue;
  }
  const flow = el('div', 'flow');
  const scroll = el('div', 'arch-scroll');
  const arch = el('div', 'arch');
  const before = isBefore(pre);
  scroll.append(arch);
  flow.append(scroll, legend(spec, before));
  pre.replaceWith(flow);
  // A raw HTML pre can itself mark Before. Its replacement only inherits the parent's side.
  flows.push({ before, spec, locate: drawFlow(arch, spec, pre.matches('.panel.before') ? isBefore(arch) : before) });
}

// Node id chips in the text open their box's files, as a click on the box does: the box in the
// flow on the same side (Before or After), or the other side when only that one has it.
const boxesById = new Map();
for (const box of out.querySelectorAll('.arch .box')) {
  const id = box.querySelector('.n')?.textContent;
  if (id === undefined) continue;
  const key = `${isBefore(box)}:${id}`;
  if (!boxesById.has(key)) boxesById.set(key, box);
}
const boxFor = (id, before) => {
  const key = `${before}:${id}`;
  const box = boxesById.get(key);
  if (!box || box.isConnected) return box;
  // Replacing a raw HTML chip can remove a box nested inside it.
  const next = [...out.querySelectorAll('.arch .box')]
    .find((b) => isBefore(b) === before && b.querySelector('.n')?.textContent === id);
  boxesById.set(key, next);
  return next;
};
for (const chip of out.querySelectorAll('span.n')) {
  if (chip.closest('.arch')) continue;
  const before = isBefore(chip);
  const box = boxFor(chip.textContent, before) || boxFor(chip.textContent, !before);
  if (!box) continue;
  const b = el('button', 'n', chip.textContent);
  b.type = 'button';
  b.setAttribute('aria-label', box.getAttribute('aria-label'));
  // A second click on a box closes its panel; a chip only opens it. From the keyboard (detail 0),
  // Enter on the box also moves focus into the panel.
  b.addEventListener('click', (ev) => {
    if (box.classList.contains('pinned')) return;
    box.opener = b;
    if (ev.detail) box.click();
    else box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
  });
  chip.replaceWith(b);
}
