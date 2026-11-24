// Inline SVG icons, icon buttons and the "open in a new tab" link.
const EYE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M1.5 8S3.8 3.5 8 3.5 14.5 8 14.5 8 12.2 12.5 8 12.5 1.5 8 1.5 8Z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><circle cx="8" cy="8" r="2" fill="currentColor"/></svg>';
const WRAP = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 4h11M2.5 8h8.5a2.25 2.25 0 0 1 0 4.5H8M9.5 11 8 12.5 9.5 14M2.5 12.5h3" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const EXTERNAL = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M9.5 2.5h4v4M13.5 2.5 8 8M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const COPY = '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M10.5 3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>';
const CHECK = '<svg class="check" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5 6.5 12 13 4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const TREE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 2.5v9a1.5 1.5 0 0 0 1.5 1.5h4M3.5 6.5H9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/><rect x="9" y="4.5" width="4.5" height="4" rx="1" fill="none" stroke="currentColor" stroke-width="1.4"/><rect x="9" y="11" width="4.5" height="4" rx="1" fill="none" stroke="currentColor" stroke-width="1.4"/></svg>';
const CLOSE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>';
function iconButton(svg, label) {
  const b = el('button', 'icon-btn');
  b.type = 'button';
  b.innerHTML = svg;
  b.title = label;
  b.setAttribute('aria-label', label);
  return b;
}
function openLink(href) {
  const a = el('a', 'icon-btn');
  a.innerHTML = EXTERNAL;
  a.href = href;
  a.target = '_blank';
  a.rel = 'noopener';
  a.title = 'Open in a new tab';
  a.setAttribute('aria-label', 'Open in a new tab');
  return a;
}
