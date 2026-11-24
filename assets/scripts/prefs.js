// Reader preference for code wrapping.
const codeWrapKey = 'overview-me:code-wrap';
try {
  document.documentElement.classList.toggle('code-wrap', localStorage.getItem(codeWrapKey) === '1');
} catch {}
// Reader preference for showing the change's other files in a box's folder tree.
const treeAllKey = 'overview-me:tree-all';
try {
  document.documentElement.classList.toggle('tree-all', localStorage.getItem(treeAllKey) === '1');
} catch {}
