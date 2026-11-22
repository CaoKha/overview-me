// Shared by every file: the config, embedded code, the markdown rendered into <main>, and DOM helpers.
// forge: https web URL of the repo or null; sha: commit of the `path:line` references or null
// (then read from "sha <40 hex>" in the header line); base: commit for references under Before;
// head: commit for previews, including when unpushed.
const cfg = JSON.parse(document.getElementById('config').textContent);
const codeFiles = JSON.parse(document.getElementById('code').textContent);
// Review findings from `overview-me findings`, or null: {source, head, findings: [{id, node?, file,
// start?, end?, severity, category, content, suggestion?}]}.
const reviewFindings = JSON.parse(document.getElementById('findings').textContent);
const SEVERITY_KIND = { critical: 'bad', high: 'bad', medium: 'warn', low: 'low' };
const findingsByNode = new Map();
for (const f of reviewFindings?.findings || []) {
  if (!f.node) continue;
  if (!findingsByNode.has(f.node)) findingsByNode.set(f.node, []);
  findingsByNode.get(f.node).push(f);
}
const md = document.getElementById('md').textContent.replace(/<\\(\/script|!--)/gi, '<$1');
const out = document.getElementById('out');
out.innerHTML = marked.parse(md);

const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};
const shortSha = (s) => s.replace(/\b([0-9a-f]{10})[0-9a-f]{30}\b/g, '$1…');
