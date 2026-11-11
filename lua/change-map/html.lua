-- Renders a store markdown file (codebase overview) as a standalone HTML page for the browser:
-- GitHub-style markdown, mermaid diagrams, and `path:line` code spans linked to the forge.
local M = {}

local function escape(s)
  return (s:gsub('&', '&amp;'):gsub('<', '&lt;'):gsub('>', '&gt;'):gsub('"', '&quot;'))
end

local page = [[<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>%s</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/github-markdown-css@5.5.1/github-markdown.min.css">
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; background: #fff; }
  @media (prefers-color-scheme: dark) { body { background: #0d1117; } }
  .markdown-body { box-sizing: border-box; max-width: 1012px; margin: 0 auto; padding: 32px 16px 64px; }
  .markdown-body pre.mermaid { background: transparent; text-align: center; }
  .markdown-body table { display: table; width: 100%%; }
  .markdown-body a code { color: inherit; }
</style>
</head>
<body>
<article class="markdown-body" id="out"></article>
<script type="text/markdown" id="md">%s</script>
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js"></script>
<script type="module">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.esm.min.mjs';
const forge = %s;
const md = document.getElementById('md').textContent.replace(/<\\(\/script|!--)/gi, '<$1');
const out = document.getElementById('out');
out.innerHTML = marked.parse(md);
// The overview's header line names the sha its `path:line` references were read at.
const sha = (md.match(/\bsha\s+([0-9a-f]{40})\b/) || [])[1];
if (forge && sha) {
  for (const code of out.querySelectorAll(':not(pre) > code')) {
    const m = code.textContent.match(/^([\w.@\/-]+\.[\w]+|[\w.@-]+(?:\/[\w.@-]+)+):(\d+)(?:-(\d+))?$/);
    if (!m) continue;
    const a = document.createElement('a');
    a.href = `${forge}/blob/${sha}/${m[1]}#L${m[2]}${m[3] ? `-L${m[3]}` : ''}`;
    a.target = '_blank';
    code.replaceWith(a);
    a.appendChild(code);
  }
}
for (const code of out.querySelectorAll('pre > code.language-mermaid')) {
  const pre = code.parentElement;
  pre.className = 'mermaid';
  pre.textContent = code.textContent;
}
const dark = matchMedia('(prefers-color-scheme: dark)').matches;
mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'default', securityLevel: 'strict' });
await mermaid.run({ querySelector: 'pre.mermaid' });
</script>
</body>
</html>
]]

-- `forge` is the repo's web URL (https://host/owner/repo) or nil.
function M.render(title, md, forge)
  -- `</script` or `<!--` in the markdown would end or break the data block; the page undoes this.
  md = md:gsub('<(/[sS][cC][rR][iI][pP][tT])', '<\\%1'):gsub('<!%-%-', '<\\!--')
  return string.format(page, escape(title), md, forge and vim.json.encode(forge) or 'null')
end

-- https web URL of the origin remote, from ssh or https remote URLs; nil when there is none.
function M.forge_url(remote)
  if not remote or remote == '' then
    return nil
  end
  local u = remote:gsub('^git@([^:]+):', 'https://%1/'):gsub('^ssh://git@([^/:]+)[:%d]*/', 'https://%1/')
  u = u:gsub('^https://[^@/]+@', 'https://'):gsub('%.git$', ''):gsub('/$', '')
  return u:match('^https://[^/]+/.+') and u or nil
end

return M
