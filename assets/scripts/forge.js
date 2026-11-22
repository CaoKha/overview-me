// `path:line` code spans link to the forge at the overview's sha, or at base under Before.
const sha = cfg.sha || (md.match(/\bsha\s+([0-9a-f]{40})\b/) || [])[1];
const PATH_RE = /^([\w.@\/$\[\]{}()+-]+\.[\w]+|[\w.@$\[\]{}()+-]+(?:\/[\w.@$\[\]{}()+-]+)+)(?::(\d+)(?:-(\d+))?)?$/;
// Forge URL of "path" or "path:line" or "path:start-end", or null; `node` decides head or base.
function forgeHref(text, node) {
  const m = text.match(PATH_RE);
  const at = cfg.base && node.closest('.panel.before') ? cfg.base : sha;
  if (!m || !cfg.forge || !at) return null;
  const lines = m[2] ? `#L${m[2]}${m[3] ? `-L${m[3]}` : ''}` : '';
  return `${cfg.forge}/blob/${at}/${m[1]}${lines}`;
}
