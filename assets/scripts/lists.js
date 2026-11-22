// Lists: numbered lists are steps, except Read first; Entry points, Watch out, Open questions and Gaps are cards.
const listKind = { 'entry points': 'cards', 'watch out': 'cards', 'open questions': 'cards', gaps: 'cards', 'read first': 'read' };
for (const list of out.querySelectorAll('section ul, section ol')) {
  if (list.parentElement.closest('li')) continue;
  const kind = listKind[list.closest('section').dataset.name];
  if (list.tagName === 'OL') list.classList.add(kind === 'read' ? 'read' : 'steps');
  else if (kind === 'cards') list.classList.add('cards');
}
for (const li of out.querySelectorAll('ol.steps > li, ol.read > li')) {
  const d = el('div');
  d.append(...li.childNodes);
  li.append(d);
}

// Watch out: a callout's leading **Tag** becomes a chip, colored by how much it can hurt.
const calloutKind = { breaking: 'bad', security: 'bad', migration: 'warn', race: 'warn', perf: 'warn', subtle: 'note' };
for (const li of out.querySelectorAll('section[data-name="watch out"] ul.cards > li')) {
  const first = li.firstElementChild;
  if (!first || first.tagName !== 'STRONG' || li.firstChild !== first) continue;
  const kind = calloutKind[first.textContent.trim().toLowerCase()];
  if (!kind) continue;
  const chip = el('span', `chip callout ${kind}`, first.textContent.trim());
  first.replaceWith(chip);
}
