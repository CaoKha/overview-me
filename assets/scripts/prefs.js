// Reader preferences for panel size, desktop position and phone sheet height.
const panelWidthKey = 'overview-me:panel-width';
const panelPositionKey = 'overview-me:panel-position';
const panelHeightKey = 'overview-me:panel-height';
const desktopHeightKey = 'overview-me:panel-desktop-height';
const codeWrapKey = 'overview-me:code-wrap';
try {
  document.documentElement.classList.toggle('code-wrap', localStorage.getItem(codeWrapKey) === '1');
} catch {}
try {
  const width = Number(localStorage.getItem(panelWidthKey));
  if (Number.isFinite(width) && width >= 320) document.documentElement.style.setProperty('--panel-width', `${width}px`);
} catch {}
let panelPosition = null;
try {
  const saved = JSON.parse(localStorage.getItem(panelPositionKey));
  if (saved && Number.isFinite(saved.x) && Number.isFinite(saved.y)) panelPosition = saved;
} catch {}
try {
  const height = Number(localStorage.getItem(panelHeightKey));
  if (Number.isFinite(height) && height >= 25 && height <= 90) document.documentElement.style.setProperty('--sheet-height', `${height}vh`);
} catch {}
try {
  const height = Number(localStorage.getItem(desktopHeightKey));
  if (Number.isFinite(height) && height >= 200) {
    document.documentElement.style.setProperty('--panel-height', `${height}px`);
    document.documentElement.classList.add('panel-sized');
  }
} catch {}
