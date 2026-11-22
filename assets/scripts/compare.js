// "### Before …" and "### After …": a panel each, with what follows up to the next heading; a
// Before panel followed by an After panel goes into one row.
for (const h of out.querySelectorAll('h3')) {
  const m = h.textContent.match(/^(before|after)\b/i);
  if (!m) continue;
  const kind = m[1].toLowerCase();
  const nodes = [h];
  for (let n = h.nextElementSibling; n && !/^H[1-3]$/.test(n.tagName); n = n.nextElementSibling) nodes.push(n);
  const panel = el('div', `panel ${kind}`);
  h.before(panel);
  // Three parts (title, diagram, text) so side-by-side panels can share row heights.
  const fig = el('div', 'panel-fig');
  const body = el('div', 'panel-body');
  const rest = nodes.slice(1);
  if (rest[0]?.matches('pre')) fig.append(rest.shift());
  body.append(...rest);
  panel.append(h, fig, body);
  h.innerHTML = h.innerHTML.replace(/^(before|after)\b\s*[:—-]?\s*/i, '');
  h.prepend(el('span', `chip ${kind === 'before' ? 'bad' : 'ok'}`, kind));
}
for (const before of out.querySelectorAll('.panel.before')) {
  const after = before.nextElementSibling;
  if (!after || !after.matches('.panel.after')) continue;
  const row = el('div', 'compare');
  before.before(row);
  row.append(before, after);
}
