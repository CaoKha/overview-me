// Reader preference for code wrapping.
const codeWrapKey = 'overview-me:code-wrap';
try {
  document.documentElement.classList.toggle('code-wrap', localStorage.getItem(codeWrapKey) === '1');
} catch {}
