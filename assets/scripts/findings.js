// Review findings (`overview-me findings`): which box holds them, its badge, and the list in its panel.
// The findings shown on a box: those of its map node, only in a diagram of head (not Before).
function boxFindings(node, arch) {
  return node.n && !arch.closest('.panel.before') ? findingsByNode.get(node.n) || [] : [];
}
function findingsSummary(list) {
  const by = {};
  for (const f of list) by[f.severity] = (by[f.severity] || 0) + 1;
  const parts = Object.keys(SEVERITY_KIND).filter((s) => by[s]).map((s) => `${by[s]} ${s}`);
  return `${parts.join(', ')} finding${list.length === 1 ? '' : 's'}`;
}
// A box's badge: its findings counted by severity, critical and high together in red.
function findingsBadge(list) {
  const counts = { bad: 0, warn: 0, low: 0 };
  for (const f of list) counts[SEVERITY_KIND[f.severity] || 'low'] += 1;
  const badge = el('span', 'findings-badge');
  badge.title = findingsSummary(list);
  for (const [kind, n] of Object.entries(counts)) if (n) badge.append(el('span', `chip ${kind}`, String(n)));
  return badge;
}
// A box panel's findings: severity, category and place, the tool's text, any suggested code, and
// the code with the finding's lines highlighted (open for the first finding).
function findingsList(list, arch) {
  const wrap = el('div', 'findings');
  wrap.append(el('p', 'detail-note', `${findingsSummary(list)} from ${reviewFindings.source}:`));
  const ul = el('ul', 'findings-list');
  list.forEach((f, i) => {
    const li = el('li', 'finding');
    const head = el('div', 'finding-head');
    const place = f.start ? `${f.file}:${f.start}${f.end > f.start ? `-${f.end}` : ''}` : f.file;
    head.append(el('span', `chip callout ${SEVERITY_KIND[f.severity] || 'low'}`, f.severity), el('span', 'finding-category', f.category), el('code', null, place));
    li.append(head);
    for (const para of f.content.split(/\n\s*\n/)) if (para.trim()) li.append(el('p', 'finding-text', para.trim()));
    if (f.suggestion) {
      const fix = el('details', 'finding-fix');
      fix.append(el('summary', null, 'Suggested code'), el('pre', null, f.suggestion));
      li.append(fix);
    }
    const preview = f.start ? codePreview(place, cfg.head || sha) : null;
    if (preview) {
      const pre = codePre(preview);
      pre.hidden = i > 0;
      const button = iconButton(EYE, pre.hidden ? 'Preview' : 'Hide preview');
      button.setAttribute('aria-label', `Preview ${place}`);
      button.setAttribute('aria-expanded', String(!pre.hidden));
      button.addEventListener('click', () => {
        pre.hidden = !pre.hidden;
        button.title = pre.hidden ? 'Preview' : 'Hide preview';
        button.setAttribute('aria-expanded', String(!pre.hidden));
        if (!pre.hidden) scrollCode(pre);
      });
      head.append(button);
      li.append(pre);
      // Scrolls to the lines once the panel is on screen.
      if (!pre.hidden) requestAnimationFrame(() => scrollCode(pre));
    }
    const href = forgeHref(place, arch);
    if (href) head.append(openLink(href));
    ul.append(li);
  });
  wrap.append(ul);
  return wrap;
}
