// Node ids (N3) in prose and tables become chips.
const walker = document.createTreeWalker(out, NodeFilter.SHOW_TEXT, {
  acceptNode: (t) => /\bN\d+\b/.test(t.data) && !t.parentElement.closest('code, pre, a, h1, .n')
    ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT,
});
const texts = [];
while (walker.nextNode()) texts.push(walker.currentNode);
for (const t of texts) {
  const frag = document.createDocumentFragment();
  let last = 0;
  for (const m of t.data.matchAll(/\bN\d+\b/g)) {
    frag.append(t.data.slice(last, m.index), el('span', 'n', m[0]));
    last = m.index + m[0].length;
  }
  frag.append(t.data.slice(last));
  t.replaceWith(frag);
}
