// Older overviews may still use mermaid; load it only when one is there.
const mermaidBlocks = out.querySelectorAll('pre > code.language-mermaid');
if (mermaidBlocks.length) {
  for (const code of mermaidBlocks) {
    const pre = code.parentElement;
    pre.className = 'mermaid';
    pre.textContent = code.textContent;
  }
  const { default: mermaid } = await import('https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.esm.min.mjs');
  const theme = document.documentElement.dataset.theme;
  const dark = theme ? theme === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
  const font = getComputedStyle(document.body);
  mermaid.initialize({
    startOnLoad: false,
    theme: dark ? 'dark' : 'default',
    securityLevel: 'strict',
    themeVariables: { fontSize: font.fontSize, fontFamily: font.fontFamily },
    flowchart: { useMaxWidth: false },
  });
  await mermaid.run({ querySelector: 'pre.mermaid' });
}
