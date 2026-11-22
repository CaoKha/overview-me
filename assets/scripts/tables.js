// Tables in cards; a cell that starts with ✓, ⚠ or ✗ becomes a status chip.
for (const t of out.querySelectorAll('table')) {
  const wrap = el('div', 'table-wrap');
  t.before(wrap);
  wrap.append(t);
}
for (const td of out.querySelectorAll('td')) {
  const s = td.textContent.trim();
  const k = s.startsWith('✓') ? 'ok' : s.startsWith('⚠') ? 'warn' : s.startsWith('✗') ? 'bad' : null;
  if (!k) continue;
  const chip = el('span', `chip ${k}`);
  chip.append(...td.childNodes);
  td.append(chip);
}
