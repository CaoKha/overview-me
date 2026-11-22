// Copies text; falls back to a selected textarea where the clipboard API is blocked (iframes).
async function copy(text, button, done = 'Copied') {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = el('textarea');
    t.value = text;
    t.style.position = 'fixed';
    t.style.opacity = '0';
    document.body.append(t);
    t.select();
    document.execCommand('copy');
    t.remove();
  }
  // A second click before the reset keeps the original content, not the "done" one.
  clearTimeout(button.copied);
  button.dataset.idle ??= button.innerHTML;
  button.innerHTML = done;
  button.copied = setTimeout(() => {
    button.innerHTML = button.dataset.idle;
    delete button.dataset.idle;
  }, 1200);
}
