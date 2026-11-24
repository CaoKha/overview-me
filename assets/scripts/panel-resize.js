// Drag, resize and remember the side panel (desktop), or the bottom sheet height (phones).
function panelResize(panel) {
  const prefix = panel.classList.contains('before') ? 'overview-me:before-panel' : 'overview-me:panel';
  const panelWidthKey = `${prefix}-width`;
  const panelPositionKey = `${prefix}-position`;
  const panelHeightKey = `${prefix}-height`;
  const desktopHeightKey = `${prefix}-desktop-height`;
  try {
    const width = Number(localStorage.getItem(panelWidthKey));
    if (Number.isFinite(width) && width >= 320) panel.style.setProperty('--panel-width', `${width}px`);
  } catch {}
  let panelPosition = null;
  try {
    const saved = JSON.parse(localStorage.getItem(panelPositionKey));
    if (saved && Number.isFinite(saved.x) && Number.isFinite(saved.y)) panelPosition = saved;
  } catch {}
  try {
    const height = Number(localStorage.getItem(panelHeightKey));
    if (Number.isFinite(height) && height >= 25 && height <= 90) panel.style.setProperty('--sheet-height', `${height}vh`);
  } catch {}
  try {
    const height = Number(localStorage.getItem(desktopHeightKey));
    if (Number.isFinite(height) && height >= 200) {
      panel.style.setProperty('--panel-height', `${height}px`);
      panel.classList.add('panel-sized');
    }
  } catch {}
  const place = () => {
    if (!panelPosition || panel.hidden || innerWidth <= 640) return;
    if (!panel.querySelector('.detail-head')) return;
    panelPosition.x = Math.max(0, Math.min(panelPosition.x, innerWidth - panel.getBoundingClientRect().width));
    panelPosition.y = Math.max(0, Math.min(panelPosition.y, innerHeight - 16 - Math.max(200, panel.getBoundingClientRect().height)));
    panel.style.setProperty('--panel-left', `${panelPosition.x}px`);
    panel.style.setProperty('--panel-top', `${panelPosition.y}px`);
  };
  const persistPosition = () => {
    try { if (panelPosition) localStorage.setItem(panelPositionKey, JSON.stringify(panelPosition)); } catch {}
  };
  const persistSize = (dx, dy) => {
    const style = panel.style;
    for (const [axis, property, key] of [[dx, '--panel-width', panelWidthKey], [dy, '--panel-height', desktopHeightKey]]) {
      if (axis && style.getPropertyValue(property)) {
        try { localStorage.setItem(key, parseFloat(style.getPropertyValue(property))); } catch {}
      }
    }
    persistPosition();
  };
  const headTarget = (ev) => ev.target.closest('.detail-head') && !ev.target.closest('button, a, .panel-resize');
  const resize = (rect, dx, dy, x, y) => {
    const size = (sign, delta, length, end, limit, min, property, key) => {
      if (!sign) return length;
      const max = Math.min(limit - 32, sign < 0 ? end : limit - (end - length) - (key === desktopHeightKey ? 16 : 0));
      const value = Math.min(Math.max(min, length + sign * delta), max);
      panel.style.setProperty(property, `${value}px`);
      return value;
    };
    const width = size(dx, x, rect.width, rect.right, innerWidth, 320, '--panel-width', panelWidthKey);
    const height = size(dy, y, rect.height, rect.bottom, innerHeight, 200, '--panel-height', desktopHeightKey);
    if (dy) panel.classList.add('panel-sized');
    panelPosition = { x: dx < 0 ? rect.right - width : rect.left, y: dy < 0 ? rect.bottom - height : rect.top };
    place();
  };
  const grips = [[-1, 0, 'w'], [0, 1, 's'], [0, -1, 'n'], [1, 0, 'e'], [1, -1, 'ne'], [-1, -1, 'nw'], [1, 1, 'se'], [-1, 1, 'sw']].map(([dx, dy, direction]) => {
    const grip = el('div', `panel-resize panel-resize-${direction}`);
    const keyboard = direction === 'w' || direction === 's';
    if (keyboard) {
      grip.tabIndex = 0;
      grip.setAttribute('role', 'separator');
      grip.setAttribute('aria-orientation', dx ? 'vertical' : 'horizontal');
      grip.setAttribute('aria-label', dx ? 'Resize panel' : 'Resize panel height');
    } else grip.setAttribute('aria-hidden', 'true');
    let drag = null;
    grip.addEventListener('pointerdown', (ev) => {
      if (ev.button !== 0 || innerWidth <= 640) return;
      ev.preventDefault();
      if (keyboard) grip.focus({ preventScroll: true });
      grip.setPointerCapture(ev.pointerId);
      drag = { id: ev.pointerId, x: ev.clientX, y: ev.clientY, rect: panel.getBoundingClientRect() };
    });
    grip.addEventListener('pointermove', (ev) => {
      if (drag?.id === ev.pointerId && innerWidth > 640) resize(drag.rect, dx, dy, ev.clientX - drag.x, ev.clientY - drag.y);
    });
    grip.addEventListener('lostpointercapture', () => {
      if (drag) persistSize(dx, dy);
      drag = null;
    });
    if (keyboard) grip.addEventListener('keydown', (ev) => {
      const keys = dx ? ['ArrowLeft', 'ArrowRight'] : ['ArrowUp', 'ArrowDown'];
      if (innerWidth <= 640 || !keys.includes(ev.key)) return;
      ev.preventDefault();
      const rect = panel.getBoundingClientRect();
      const side = dx ? (rect.left > innerWidth - rect.right ? -1 : 1) : (rect.top > innerHeight - 16 - rect.bottom ? -1 : 1);
      const delta = side * (keys.indexOf(ev.key) ? 32 : -32);
      resize(rect, dx ? side : 0, dy ? side : 0, dx ? delta : 0, dy ? delta : 0);
      persistSize(dx, dy);
    });
    grip.addEventListener('dblclick', () => {
      if (dx) {
        panel.style.removeProperty('--panel-width');
        try { localStorage.removeItem(panelWidthKey); } catch {}
      }
      if (dy) {
        panel.style.removeProperty('--panel-height');
        panel.classList.remove('panel-sized');
        try { localStorage.removeItem(desktopHeightKey); } catch {}
      }
      place();
      persistPosition();
    });
    return grip;
  });
  const updateAria = () => {
    for (const grip of grips.filter((g) => g.tabIndex === 0)) {
      const width = grip.getAttribute('aria-orientation') === 'vertical';
      grip.setAttribute('aria-valuemin', width ? 320 : 200);
      grip.setAttribute('aria-valuemax', Math.max(width ? 320 : 200, (width ? innerWidth : innerHeight) - 32));
      grip.setAttribute('aria-valuenow', Math.round(panel.getBoundingClientRect()[width ? 'width' : 'height']));
    }
  };
  let move = null;
  panel.addEventListener('pointerdown', (ev) => {
    if (ev.button !== 0 || !headTarget(ev)) return;
    ev.preventDefault();
    const rect = panel.getBoundingClientRect();
    move = { id: ev.pointerId, x: ev.clientX, y: ev.clientY, left: rect.left, top: rect.top, height: rect.height, phone: innerWidth <= 640 };
    ev.target.closest('.detail-head').setPointerCapture(ev.pointerId);
    panel.classList.add('dragging');
  });
  panel.addEventListener('pointermove', (ev) => {
    if (move?.id !== ev.pointerId) return;
    if (move.phone) {
      const height = Math.max(25, Math.min(90, (move.height + move.y - ev.clientY) / innerHeight * 100));
      panel.style.setProperty('--sheet-height', `${height}vh`);
    } else {
      panelPosition = { x: move.left + ev.clientX - move.x, y: move.top + ev.clientY - move.y };
      place();
    }
  });
  panel.addEventListener('lostpointercapture', (ev) => {
    if (move?.id !== ev.pointerId) return;
    if (move.phone) {
      const height = panel.style.getPropertyValue('--sheet-height');
      try { if (height) localStorage.setItem(panelHeightKey, parseFloat(height)); } catch {}
    } else persistPosition();
    move = null;
    panel.classList.remove('dragging');
  });
  panel.addEventListener('dblclick', (ev) => {
    if (!headTarget(ev)) return;
    if (innerWidth <= 640) {
      panel.style.removeProperty('--sheet-height');
      try { localStorage.removeItem(panelHeightKey); } catch {}
    } else {
      panelPosition = null;
      panel.style.removeProperty('--panel-left');
      panel.style.removeProperty('--panel-top');
      try { localStorage.removeItem(panelPositionKey); } catch {}
    }
  });
  new ResizeObserver(() => { place(); updateAria(); }).observe(panel);
  window.addEventListener('resize', () => { place(); updateAria(); });
  return grips;
}
// The bar between a box's folder tree and its code: drag it or use the arrow keys to set the tree's
// width, double-click for the default. Both panels share the remembered width.
const treeWidthKey = 'overview-me:tree-width';
try {
  const width = Number(localStorage.getItem(treeWidthKey));
  if (Number.isFinite(width) && width >= 120) document.documentElement.style.setProperty('--tree-width', `${width}px`);
} catch {}
function treeResize(split) {
  const min = 120;
  const grip = el('div', 'tree-resize');
  grip.tabIndex = 0;
  grip.setAttribute('role', 'separator');
  grip.setAttribute('aria-orientation', 'vertical');
  grip.setAttribute('aria-label', 'Resize folder tree');
  grip.setAttribute('aria-valuemin', min);
  const tree = () => split.querySelector('.file-tree');
  // The code keeps at least 240px.
  const max = () => Math.max(min, split.clientWidth - 240);
  const report = () => {
    grip.setAttribute('aria-valuemax', max());
    grip.setAttribute('aria-valuenow', Math.round(tree().getBoundingClientRect().width));
  };
  grip.addEventListener('focus', report);
  const setWidth = (width) => {
    const style = document.documentElement.style;
    if (width === null) style.removeProperty('--tree-width');
    else style.setProperty('--tree-width', `${Math.round(Math.min(Math.max(min, width), max()))}px`);
    report();
    try {
      if (width === null) localStorage.removeItem(treeWidthKey);
      else localStorage.setItem(treeWidthKey, String(Math.round(tree().getBoundingClientRect().width)));
    } catch {}
  };
  let drag = null;
  grip.addEventListener('pointerdown', (ev) => {
    if (ev.button !== 0) return;
    ev.preventDefault();
    ev.stopPropagation();
    grip.focus({ preventScroll: true });
    grip.setPointerCapture(ev.pointerId);
    drag = { id: ev.pointerId, x: ev.clientX, width: tree().getBoundingClientRect().width };
  });
  grip.addEventListener('pointermove', (ev) => {
    if (drag?.id === ev.pointerId) setWidth(drag.width + ev.clientX - drag.x);
  });
  grip.addEventListener('lostpointercapture', () => { drag = null; });
  grip.addEventListener('keydown', (ev) => {
    const step = { ArrowLeft: -16, ArrowRight: 16 }[ev.key];
    if (!step) return;
    ev.preventDefault();
    setWidth(tree().getBoundingClientRect().width + step);
  });
  grip.addEventListener('dblclick', () => setWidth(null));
  return grip;
}
