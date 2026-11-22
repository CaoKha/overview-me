// One section per `##`, named by its heading for section-specific styles.
for (const h2 of out.querySelectorAll('h2')) {
  const sec = el('section');
  sec.dataset.name = h2.textContent.trim().toLowerCase();
  const nodes = [h2];
  for (let n = h2.nextElementSibling; n && n.tagName !== 'H2'; n = n.nextElementSibling) nodes.push(n);
  h2.before(sec);
  sec.append(...nodes);
}
