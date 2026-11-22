// Header: "# Name · kind" becomes a tag line over the title; the "a · b · c" line under it, pills.
const h1 = out.querySelector('h1');
if (h1) {
  const header = el('header');
  h1.before(header);
  const parts = h1.textContent.split(' · ');
  if (parts.length > 1) {
    header.append(el('span', 'tag', parts.pop()));
    h1.textContent = parts.join(' · ');
  }
  header.append(h1);
  // "14 stops", "spec: none", "sha <hex>", "base...head" and dates become labelled facts.
  const meta = header.nextElementSibling;
  if (meta && meta.tagName === 'P' && meta.textContent.includes(' · ')) {
    const facts = el('div', 'facts');
    for (const raw of meta.textContent.split(' · ')) {
      const p = raw.trim();
      const m = p.match(/^(\d+)\s+(.+)$/) || p.match(/^([a-z][\w ]*):\s*(.+)$/i);
      const [label, value] = /^\d+\s/.test(p) ? [m[2], m[1]]
        : m ? [m[1], m[2]]
        : /^sha\s/.test(p) ? ['commit', p.slice(4)]
        : p.includes('...') ? ['commits', p]
        : /^\d{4}-\d{2}-\d{2}$/.test(p) ? ['date', p]
        : [/\s/.test(p) ? 'scope' : 'branch', p];
      const fact = el('div', value.length > 14 ? 'fact wide' : 'fact');
      fact.append(el('span', 'fact-label', label[0].toUpperCase() + label.slice(1)), el('span', 'fact-value', shortSha(value)));
      facts.append(fact);
    }
    meta.remove();
    header.append(facts);
  }
}
