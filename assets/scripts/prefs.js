// Reader preference for code wrapping.
const codeWrapKey = 'overview-me:code-wrap';
// Reader preference for showing the change's other files in a box's folder tree.
const treeAllKey = 'overview-me:tree-all';
for (const [cls, key] of [['code-wrap', codeWrapKey], ['tree-all', treeAllKey]]) {
  try {
    document.documentElement.classList.toggle(cls, localStorage.getItem(key) === '1');
  } catch {}
}
