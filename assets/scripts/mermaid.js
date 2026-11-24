// Older overviews may still use mermaid; load it only when one is there.
const mermaidBlocks = out.querySelectorAll('pre > code.language-mermaid');
if (mermaidBlocks.length) {
  const sources = new Map();
  for (const code of mermaidBlocks) {
    const pre = code.parentElement;
    pre.className = 'mermaid';
    pre.textContent = code.textContent;
    sources.set(pre, code.textContent);
  }
  const { default: mermaid } = await import('https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.esm.min.mjs');
  // Tokens hold light-dark(), and palettes may use oklch() or color-mix(); mermaid reads only
  // hex, rgb and hsl. A probe resolves the token, a 1px canvas turns the result into rgb().
  const probe = el('span');
  probe.hidden = true;
  document.body.append(probe);
  const pixel = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  const token = (name) => {
    probe.style.color = `var(--${name})`;
    const color = getComputedStyle(probe).color;
    if (color.startsWith('rgb')) return color; // skip the canvas: some browsers randomise readback
    pixel.clearRect(0, 0, 1, 1);
    pixel.fillStyle = color;
    pixel.fillRect(0, 0, 1, 1);
    const [r, g, b] = pixel.getImageData(0, 0, 1, 1).data;
    return `rgb(${r}, ${g}, ${b})`;
  };
  const draw = async () => {
    const theme = document.documentElement.dataset.theme;
    const dark = theme ? theme === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
    const font = getComputedStyle(document.body);
    const [card, popover, fg, edge] = ['card', 'popover', 'fg', 'edge'].map(token);
    mermaid.initialize({
      startOnLoad: false,
      theme: 'base',
      securityLevel: 'strict',
      themeVariables: {
        fontSize: font.fontSize, fontFamily: font.fontFamily, darkMode: dark,
        background: card, clusterBkg: card, edgeLabelBackground: card, tertiaryColor: card,
        primaryColor: popover, mainBkg: popover,
        primaryTextColor: fg, textColor: fg,
        primaryBorderColor: edge, nodeBorder: edge,
        clusterBorder: token('border'), secondaryColor: token('secondary'), lineColor: token('muted'),
      },
      flowchart: { useMaxWidth: false },
    });
    for (const [pre, source] of sources) {
      pre.removeAttribute('data-processed');
      pre.textContent = source;
    }
    await mermaid.run({ querySelector: 'pre.mermaid' });
  };
  document.addEventListener('palettechange', draw);
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', draw);
  await draw();
}
