// Section bar: a link per section; the last section whose top has passed the bar is current.
const sections = [...out.querySelectorAll('section')];
if (sections.length > 1) {
  const toc = el('nav', 'toc');
  toc.setAttribute('aria-label', 'Sections');
  const track = el('div', 'toc-track');
  const links = sections.map((sec) => {
    sec.id = sec.dataset.name.replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
    const a = el('a', null, sec.querySelector('h2').textContent.trim());
    a.href = `#${sec.id}`;
    track.append(a);
    return a;
  });
  toc.append(el('p', 'toc-title', 'On this page'), track);
  // After the header: the bar, then the sections. On a wide screen the bar becomes the left rail.
  const content = el('div', 'content');
  content.append(...sections);
  out.append(toc, content);
  let current = -1;
  const rail = matchMedia('(min-width: 1200px)');
  const spy = () => {
    const line = rail.matches ? 120 : toc.getBoundingClientRect().bottom + 24;
    let i = 0;
    sections.forEach((sec, k) => { if (sec.getBoundingClientRect().top <= line) i = k; });
    if (innerHeight + scrollY >= document.documentElement.scrollHeight - 2) i = sections.length - 1;
    if (i === current) return;
    links[current]?.removeAttribute('aria-current');
    links[i].setAttribute('aria-current', 'true');
    current = i;
    if (!rail.matches) track.scrollTo({ left: links[i].offsetLeft - (track.clientWidth - links[i].offsetWidth) / 2 });
  };
  let queued = false;
  addEventListener('scroll', () => { if (!queued) { queued = true; requestAnimationFrame(() => { queued = false; spy(); }); } }, { passive: true });
  addEventListener('resize', spy);
  spy();
}
