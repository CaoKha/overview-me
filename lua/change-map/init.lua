-- change-map: reads <git-common-dir>/change-map/<head>.json and links it to the code:
-- a quickfix tour, captions above every site, a map panel, and ]x/[x between a node's sites.
local M = {}

local ns = vim.api.nvim_create_namespace('change-map')
local group = vim.api.nvim_create_augroup('change-map', { clear = true })

local state -- decoded map + repo, by_id, by_file, panel_buf

local function norm(path)
  return vim.fs.normalize(vim.fn.resolve(vim.fn.fnamemodify(path, ':p')))
end

local function git(args)
  local out = vim.fn.systemlist(vim.list_extend({ 'git' }, args))
  if vim.v.shell_error ~= 0 then
    return nil
  end
  return out
end

local function mark(c)
  if c.status == 'missing' then
    return '✗'
  end
  if c.status == 'implemented' and #c.tests > 0 then
    return '✓'
  end
  return '⚠'
end

local function tags(n)
  local refs = #n.criteria > 0 and table.concat(n.criteria, ',') or 'no criterion'
  return string.format('%d/%d · %s · %s · %s', n.order, #state.tour, n.lane, n.risk, refs)
end

local function annotate(buf)
  if not state or not vim.api.nvim_buf_is_loaded(buf) then
    return
  end
  local entries = state.by_file[norm(vim.api.nvim_buf_get_name(buf))]
  if not entries then
    return
  end
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  local count = vim.api.nvim_buf_line_count(buf)
  for _, e in ipairs(entries) do
    local n = e.node
    local text = e.k == 1 and string.format('▎ %s · %s', tags(n), n.caption)
      or string.format('▎ %d/%d · site %d/%d · %s', n.order, #state.tour, e.k, #n.sites, n.symbol)
    vim.api.nvim_buf_set_extmark(buf, ns, math.min(e.site.start, count) - 1, 0, {
      virt_lines = { { { text, n.risk == 'read' and 'DiagnosticVirtualTextWarn' or 'Comment' } } },
      virt_lines_above = true,
    })
  end
  vim.keymap.set('n', ']x', function()
    M.site(1)
  end, { buffer = buf, desc = 'Next site of this review stop' })
  vim.keymap.set('n', '[x', function()
    M.site(-1)
  end, { buffer = buf, desc = 'Previous site of this review stop' })
end

local function open_site(site, avoid_win)
  if avoid_win then
    local target
    for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
      if win ~= avoid_win and vim.bo[vim.api.nvim_win_get_buf(win)].buftype == '' then
        target = win
        break
      end
    end
    if target then
      vim.api.nvim_set_current_win(target)
    else
      vim.cmd('botright vsplit')
    end
  end
  vim.cmd.edit(vim.fn.fnameescape(site.path))
  vim.api.nvim_win_set_cursor(0, { math.min(site.start, vim.api.nvim_buf_line_count(0)), 0 })
  vim.cmd('normal! zz')
end

local function list_of(v, ok)
  if type(v) ~= 'table' or not vim.islist(v) then
    return false
  end
  for _, x in ipairs(v) do
    if type(x) ~= 'table' or not ok(x) then
      return false
    end
  end
  return true
end

local function str(...)
  for _, v in ipairs({ ... }) do
    if type(v) ~= 'string' then
      return false
    end
  end
  return true
end

-- The fields and ids this plugin uses; `change-map validate` checks the rest.
function M.shape_ok(map)
  if type(map) ~= 'table' or not str(map.base) or type(map.tour) ~= 'table' or not vim.islist(map.tour) then
    return false
  end
  local ids = {}
  local nodes_ok = list_of(map.nodes, function(n)
    local optional = function(v, f)
      return v == nil or list_of(v, f)
    end
    if not str(n.id, n.symbol, n.lane, n.risk, n.caption) or ids[n.id] then
      return false
    end
    ids[n.id] = true
    return list_of(n.sites, function(s)
      return str(s.file) and type(s.start) == 'number'
    end) and type(n.criteria) == 'table' and optional(n.deletions, function(d)
      return str(d.file)
    end) and optional(n.renames, function(r)
      return str(r.from, r.to)
    end)
  end)
  local criteria_ok = list_of(map.criteria, function(c)
    return str(c.id, c.text, c.status) and list_of(c.tests, function(t)
      return str(t.file, t.name) and type(t.line) == 'number'
    end)
  end)
  local edges_ok = list_of(map.edges, function(e)
    return ids[e.from] and ids[e.to] and str(e.kind)
  end)
  if not (nodes_ok and criteria_ok and edges_ok) then
    return false
  end
  local seen = {}
  for _, id in ipairs(map.tour) do
    if not ids[id] or seen[id] then
      return false
    end
    seen[id] = true
  end
  return vim.tbl_count(seen) == vim.tbl_count(ids)
end

function M.load(path)
  local top = git({ 'rev-parse', '--show-toplevel' })
  if not top then
    vim.notify('change-map: not in a git repo', vim.log.levels.ERROR)
    return false
  end
  if path == '' then
    -- `change-map path` is the source of truth; fall back to the same rule when it is not built.
    local bin = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':h:h:h') .. '/target/release/change-map'
    local out = vim.fn.executable(bin) == 1 and vim.fn.systemlist({ bin, 'path' }) or {}
    if vim.v.shell_error == 0 and out[1] then
      path = out[1]
    else
      local r = git({ 'rev-parse', '--path-format=absolute', '--git-common-dir', 'HEAD' }) or {}
      path = string.format('%s/change-map/%s.json', r[1], r[2])
    end
  end
  path = norm(path)
  if vim.fn.filereadable(path) == 0 then
    vim.notify('change-map: no map at ' .. path .. ' (run the change-map skill first)', vim.log.levels.ERROR)
    return false
  end
  -- A map the agent is still writing can be half-done: check what this file reads before
  -- touching anything, and keep the current map if it fails.
  local ok, decoded = pcall(vim.json.decode, table.concat(vim.fn.readfile(path), '\n'))
  if not ok or not M.shape_ok(decoded) then
    vim.notify('change-map: change map is incomplete or invalid; run change-map validate', vim.log.levels.WARN)
    return false
  end
  -- Drop the old map's panel before replacing state, or a second load leaves an orphan picker
  -- and a panel buffer whose name blocks the next one (E95).
  if state then
    if state.picker and not state.picker.closed then
      state.picker:close()
    end
    if state.panel_buf and vim.api.nvim_buf_is_valid(state.panel_buf) then
      vim.api.nvim_buf_delete(state.panel_buf, { force = true })
    end
  end
  state = decoded
  state.path, state.repo, state.by_id, state.by_file = path, top[1], {}, {}
  for _, c in ipairs(state.criteria) do
    for _, t in ipairs(c.tests) do
      t.path = norm(state.repo .. '/' .. t.file)
    end
  end
  for _, n in ipairs(state.nodes) do
    n.deletions, n.renames = n.deletions or {}, n.renames or {}
    for _, r in ipairs(n.renames) do
      r.path = norm(state.repo .. '/' .. r.to)
    end
    -- Where the node opens: its first site, else the new path of its first move.
    local r = n.renames[1]
    n.target = n.sites[1] or (r and { path = r.path, start = 1 })
    state.by_id[n.id] = n
    for k, site in ipairs(n.sites) do
      site.path = norm(state.repo .. '/' .. site.file)
      state.by_file[site.path] = state.by_file[site.path] or {}
      table.insert(state.by_file[site.path], { node = n, site = site, k = k })
    end
  end
  local items = {}
  for i, id in ipairs(state.tour) do
    local n = state.by_id[id]
    n.order = i
    local text = string.format('[%s] %s', tags(n), n.caption)
    local site = n.target
    -- A node that only deletes files has no line to jump to: a text-only entry keeps the count.
    table.insert(items, site and { filename = site.path, lnum = site.start, col = 1, text = text } or { text = text })
  end
  vim.fn.setqflist({}, ' ', { title = 'Change map', items = items })
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    -- Clear every buffer first: files that the previous map annotated may not be in this one.
    if vim.api.nvim_buf_is_loaded(buf) then
      vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
    end
    annotate(buf)
  end
  vim.api.nvim_clear_autocmds({ group = group })
  vim.api.nvim_create_autocmd('BufReadPost', {
    group = group,
    callback = function(ev)
      annotate(ev.buf)
    end,
  })
  vim.notify(string.format('change-map: %d stops, %d criteria. ]q walks the tour, ]x the sites, :ChangeMapPanel', #state.tour, #state.criteria))
  return true
end

-- Jump to the next (dir 1) or previous (-1) site of the node under or above the cursor.
function M.site(dir)
  local entries = state and state.by_file[norm(vim.api.nvim_buf_get_name(0))] or {}
  local line, cur = vim.fn.line('.'), nil
  for _, e in ipairs(entries) do
    if e.site.start <= line and (not cur or e.site.start > cur.site.start) then
      cur = e
    end
  end
  cur = cur or entries[1]
  if not cur then
    return
  end
  local sites = cur.node.sites
  local k = (cur.k - 1 + dir) % #sites + 1
  vim.fn.setqflist({}, 'a', { idx = cur.node.order })
  open_site(sites[k])
  vim.notify(string.format('%s: site %d/%d', cur.node.symbol, k, #sites))
end

-- The panel: a snacks picker tree like the explorer when snacks is installed, else a plain buffer.
function M.panel(opts)
  if not state then
    vim.notify('change-map: run :ChangeMap first', vim.log.levels.ERROR)
    return
  end
  local has_snacks, snacks = pcall(require, 'snacks')
  if has_snacks and snacks.picker then
    if state.picker and not state.picker.closed then
      state.picker:close()
    end
    state.picker = require('change-map.picker').open(state, opts)
    return
  end
  local lines, targets, heads = {}, {}, {}
  local function add(text, site, head)
    table.insert(lines, text)
    targets[#lines], heads[#lines] = site, head
  end
  local function node_line(n)
    add(string.format('      %d %s %s', n.order, n.risk == 'read' and '●' or '○', n.symbol), n.target)
  end
  local tour = vim.tbl_map(function(id)
    return state.by_id[id]
  end, state.tour)

  add('Spec', nil, true)
  for _, c in ipairs(state.criteria) do
    local note = c.status ~= 'implemented' and c.status or (#c.tests > 0 and '' or 'untested')
    add(string.format('  %s %s %s%s', mark(c), c.id, c.text, note ~= '' and ' (' .. note .. ')' or ''))
    for _, n in ipairs(tour) do
      if vim.tbl_contains(n.criteria, c.id) then
        node_line(n)
      end
    end
    for _, t in ipairs(c.tests) do
      add(string.format('      test %s  %s:%d', t.name, t.file:match('[^/]+$'), t.line), { path = t.path, start = t.line })
    end
  end
  add('')
  add('Lanes', nil, true)
  for _, lane in ipairs({ 'ui', 'api', 'services', 'db', 'engine', 'jobs', 'other' }) do
    local in_lane = vim.tbl_filter(function(n)
      return n.lane == lane
    end, tour)
    if #in_lane > 0 then
      add('  ' .. lane)
    end
    for _, n in ipairs(in_lane) do
      node_line(n)
      for _, d in ipairs(n.deletions) do
        add('          ✗ deleted ' .. d.file)
      end
      for _, r in ipairs(n.renames) do
        add('          → moved from ' .. r.from, { path = r.path, start = 1 })
      end
      for _, s in ipairs(n.sites) do
        add(string.format('          %s:%d', s.file:match('[^/]+/[^/]+$') or s.file, s.start), s)
      end
      for _, e in ipairs(state.edges) do
        if e.from == n.id then
          add(string.format('          → %s %s', e.kind, state.by_id[e.to].symbol), state.by_id[e.to].target)
        end
      end
    end
  end
  add('')
  add('Unexplained', nil, true)
  for _, n in ipairs(tour) do
    if #n.criteria == 0 then
      node_line(n)
    end
  end

  if state.panel_buf and vim.api.nvim_buf_is_valid(state.panel_buf) then
    vim.api.nvim_buf_delete(state.panel_buf, { force = true })
  end
  vim.cmd('topleft 60vnew')
  local buf, win = vim.api.nvim_get_current_buf(), vim.api.nvim_get_current_win()
  state.panel_buf = buf
  vim.api.nvim_buf_set_name(buf, 'change-map://panel')
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  for i in pairs(heads) do
    vim.api.nvim_buf_set_extmark(buf, ns, i - 1, 0, { end_col = #lines[i], hl_group = 'Title' })
  end
  vim.bo[buf].buftype, vim.bo[buf].bufhidden, vim.bo[buf].swapfile = 'nofile', 'wipe', false
  vim.bo[buf].modifiable, vim.bo[buf].filetype, vim.bo[buf].shiftwidth = false, 'changemap', 2
  vim.wo[win].foldmethod, vim.wo[win].foldlevel = 'indent', 99
  vim.wo[win].number, vim.wo[win].relativenumber, vim.wo[win].signcolumn = false, false, 'no'
  vim.wo[win].foldcolumn, vim.wo[win].statuscolumn = '0', ''
  vim.wo[win].wrap, vim.wo[win].linebreak, vim.wo[win].breakindent = true, true, true
  vim.wo[win].breakindentopt, vim.wo[win].winfixwidth, vim.wo[win].cursorline = 'shift:4', true, true
  local opts = { buffer = buf, nowait = true }
  vim.keymap.set('n', '<CR>', function()
    local site = targets[vim.api.nvim_win_get_cursor(0)[1]]
    if site then
      open_site(site, win)
    end
  end, opts)
  vim.keymap.set('n', 'o', 'za', opts)
  vim.keymap.set('n', 'q', '<cmd>close<cr>', opts)
end

function M.toggle_panel()
  if state and state.picker and not state.picker.closed then
    state.picker:close()
  elseif state and state.panel_buf and vim.fn.bufwinid(state.panel_buf) ~= -1 then
    vim.api.nvim_win_close(vim.fn.bufwinid(state.panel_buf), true)
  else
    M.panel()
  end
end

-- Loads the map for HEAD unless one is loaded; for keymaps.
function M.ensure()
  return state ~= nil or M.load('')
end

-- Next (1) or previous (-1) tour stop.
function M.stop(dir)
  if M.ensure() then
    pcall(vim.cmd, dir > 0 and 'cnext' or 'cprevious')
  end
end

-- Open a markdown file from the store read-only; its repo-relative paths resolve with gF.
local function open_doc(p, repo, skill)
  if vim.fn.filereadable(p) == 0 then
    vim.notify('change-map: nothing at ' .. p .. ' (run the ' .. skill .. ' skill first)', vim.log.levels.ERROR)
    return
  end
  vim.cmd('vsplit ' .. vim.fn.fnameescape(p))
  vim.bo.path = repo .. ',' .. vim.o.path
  vim.bo.readonly = true
end

-- Open the codebase overview: one per repo, needs no map. In the browser by default, rendered
-- to <store>/codebase.html with diagrams and links to the forge; { split = true } opens the
-- markdown in a split instead, where gF jumps to the code.
function M.codebase(opts)
  local r = git({ 'rev-parse', '--path-format=absolute', '--git-common-dir', '--show-toplevel' })
  if not r then
    vim.notify('change-map: not in a git repo', vim.log.levels.ERROR)
    return
  end
  local md = r[1] .. '/change-map/codebase.md'
  if (opts or {}).split or vim.fn.filereadable(md) == 0 then
    return open_doc(md, r[2], 'codebase-overview')
  end
  local html = require('change-map.html')
  local remote = (git({ '-C', r[2], 'remote', 'get-url', 'origin' }) or {})[1]
  local title = vim.fn.fnamemodify(r[2], ':t') .. ' · codebase overview'
  local out = r[1] .. '/change-map/codebase.html'
  local text = table.concat(vim.fn.readfile(md), '\n')
  if vim.fn.writefile(vim.split(html.render(title, text, html.forge_url(remote)), '\n'), out) ~= 0 then
    vim.notify('change-map: cannot write ' .. out, vim.log.levels.ERROR)
    return
  end
  local _, err = vim.ui.open(out)
  if err then
    vim.notify('change-map: ' .. err .. '; opening the markdown instead', vim.log.levels.WARN)
    open_doc(md, r[2], 'codebase-overview')
  end
end

-- Open the feature overview that the feature-overview skill writes next to the map.
function M.overview()
  if not M.ensure() then
    return
  end
  -- Paths in the overview are repo-relative: gF resolves them against the repo, not the store.
  open_doc((state.path:gsub('%.json$', '.overview.md')), state.repo, 'feature-overview')
end

function M.diff()
  if not state then
    vim.notify('change-map: run :ChangeMap first', vim.log.levels.ERROR)
    return
  end
  if vim.bo.buftype ~= '' then
    vim.notify('change-map: :ChangeMapDiff needs a file buffer', vim.log.levels.ERROR)
    return
  end
  local ok, gs = pcall(require, 'gitsigns')
  if ok then
    gs.change_base(state.base, true) -- signs and ]h hunks now show the whole change
    gs.diffthis(state.base)
    return
  end
  local rel = norm(vim.api.nvim_buf_get_name(0)):sub(#norm(state.repo) + 2)
  local old = git({ '-C', state.repo, 'show', state.base .. ':' .. rel }) or {} -- empty when the file is new
  local ft = vim.bo.filetype
  vim.cmd('diffthis')
  vim.cmd('vnew')
  vim.api.nvim_buf_set_lines(0, 0, -1, false, old)
  vim.bo.buftype, vim.bo.bufhidden, vim.bo.modifiable, vim.bo.filetype = 'nofile', 'wipe', false, ft
  vim.cmd('diffthis | wincmd p')
end

return M
