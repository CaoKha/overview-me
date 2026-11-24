// Reader preference for the color palette: every `:root[data-palette="<name>"]` rule on the page
// (assets/themes/ and `overview-me page --theme`) is a choice; Default is tokens.css alone.
const paletteKey = 'overview-me:palette';
const palettes = new Set();
const findPalettes = (rules) => {
  for (const rule of rules) {
    for (const m of (rule.selectorText || '').matchAll(/data-palette="([^"]+)"/g)) palettes.add(m[1]);
    if (rule.cssRules) findPalettes(rule.cssRules); // inside @media, @supports, @layer
  }
};
for (const sheet of document.styleSheets) {
  try { findPalettes(sheet.cssRules); } catch {} // cross-origin sheets (Google Fonts) throw
}
const setPalette = (name) => {
  if (palettes.has(name)) document.documentElement.dataset.palette = name;
  else delete document.documentElement.dataset.palette;
};
try { setPalette(localStorage.getItem(paletteKey)); } catch {}
if (palettes.size) {
  const picker = el('select', 'palette');
  picker.setAttribute('aria-label', 'Color palette');
  for (const name of ['', ...palettes]) {
    const option = el('option', null, name ? name.replace(/(^|-)(.)/g, (_, dash, c) => (dash && ' ') + c.toUpperCase()) : 'Default');
    option.value = name;
    picker.append(option);
  }
  picker.value = document.documentElement.dataset.palette || '';
  picker.addEventListener('change', () => {
    setPalette(picker.value);
    try { localStorage.setItem(paletteKey, picker.value); } catch {}
    document.dispatchEvent(new Event('palettechange'));
  });
  (out.querySelector('header') || out).append(picker);
}
