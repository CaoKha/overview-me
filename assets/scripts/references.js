// `path:line` code spans in the text: a forge link and a preview button.
for (const code of out.querySelectorAll(':not(pre) > code')) {
  if (!/:\d+(-\d+)?$/.test(code.textContent)) continue;
  const at = cfg.base && code.closest('.panel.before') ? cfg.base : cfg.head || sha;
  const preview = codePreview(code.textContent, at);
  const href = forgeHref(code.textContent, code);
  let reference = code;
  if (href) {
    const a = el('a');
    a.href = href;
    a.target = '_blank';
    a.rel = 'noopener';
    code.replaceWith(a);
    a.append(code);
    reference = a;
  }
  if (preview) {
    const button = el('button', 'peek');
    button.type = 'button';
    button.title = 'Preview';
    button.innerHTML = EYE;
    button.setAttribute('aria-label', `Preview ${code.textContent}`);
    button.setAttribute('aria-expanded', 'false');
    const before = !!code.closest('.panel.before');
    button.addEventListener('click', () => showCode(code.textContent, preview, href, button, before));
    reference.after(button);
  }
}
