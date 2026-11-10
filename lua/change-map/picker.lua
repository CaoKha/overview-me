-- The change map panel as a snacks picker tree, set up like the snacks explorer:
-- Spec → criteria → nodes, Lanes → nodes → sites, Unexplained. Folders fold, typing filters.
local P = {}

local status_hl = { ok = 'DiagnosticOk', warn = 'DiagnosticWarn', missing = 'DiagnosticError' }
local open_state = {} -- key -> false when folded; survives reopening the panel

local function criterion_status(c)
  if c.status == 'missing' then
    return '✗', status_hl.missing
  end
  if c.status == 'implemented' and #c.tests > 0 then
    return '✓', status_hl.ok
  end
  return '⚠', status_hl.warn
end

local function file_icon(path)
  local ok, icons = pcall(require, 'mini.icons')
  if not ok then
    return nil
  end
  local icon, hl = icons.get('file', path)
  return icon, hl
end

-- Builds the whole tree once; the finder emits the open part of it.
local function build(state)
  local roots = {}
  local function item(parent, it)
    it.parent, it.children = parent, {}
    table.insert(parent and parent.children or roots, it)
    return it
  end
  local function node_item(parent, n, key, with_sites)
    local site = n.target
    local it = item(parent, {
      kind = 'node',
      key = key,
      node = n,
      file = site and site.path,
      pos = site and { site.start, 0 },
      text = table.concat({ n.order, n.symbol, n.caption, n.lane, n.risk }, ' '),
    })
    for k, r in ipairs(with_sites and n.renames or {}) do
      item(it, { kind = 'rename', key = key .. '/moved/' .. k, node = n, rename = r, file = r.path, pos = { 1, 0 }, text = 'moved from ' .. r.from })
    end
    for k, d in ipairs(with_sites and n.deletions or {}) do
      item(it, { kind = 'deletion', key = key .. '/deleted/' .. k, deletion = d, text = 'deleted ' .. d.file })
    end
    if with_sites and #n.sites > 1 then
      for k, s in ipairs(n.sites) do
        item(it, { kind = 'site', key = key .. '/' .. k, node = n, site = s, k = k, file = s.path, pos = { s.start, 0 }, text = s.file })
      end
    end
    return it
  end
  local tour = vim.tbl_map(function(id)
    return state.by_id[id]
  end, state.tour)

  local spec = item(nil, { kind = 'section', key = 'spec', label = 'Spec', text = 'Spec' })
  for _, c in ipairs(state.criteria) do
    local ci = item(spec, { kind = 'criterion', key = c.id, criterion = c, text = c.id .. ' ' .. c.text })
    for _, n in ipairs(tour) do
      if vim.tbl_contains(n.criteria, c.id) then
        node_item(ci, n, c.id .. '/' .. n.id, false)
      end
    end
    for k, t in ipairs(c.tests) do
      item(ci, { kind = 'test', key = c.id .. '/test/' .. k, test = t, file = t.path, pos = { t.line, 0 }, text = 'test ' .. t.name })
    end
  end
  local lanes = item(nil, { kind = 'section', key = 'lanes', label = 'Lanes', text = 'Lanes' })
  for _, lane in ipairs({ 'ui', 'api', 'services', 'db', 'engine', 'jobs', 'other' }) do
    local li
    for _, n in ipairs(tour) do
      if n.lane == lane then
        li = li or item(lanes, { kind = 'lane', key = 'lane/' .. lane, label = lane, text = lane })
        local ni = node_item(li, n, 'lane/' .. n.id, true)
        for _, e in ipairs(state.edges) do
          if e.from == n.id then
            local to = state.by_id[e.to]
            -- The callee may only delete files: then the edge is text, with no jump.
            local t = to.target
            item(ni, { kind = 'edge', key = 'edge/' .. n.id .. '/' .. e.to, edge = e, to = to, file = t and t.path, pos = t and { t.start, 0 }, text = e.kind .. ' ' .. to.symbol })
          end
        end
      end
    end
  end
  local unexplained = item(nil, { kind = 'section', key = 'unexplained', label = 'Unexplained', text = 'Unexplained' })
  for _, n in ipairs(tour) do
    if #n.criteria == 0 then
      node_item(unexplained, n, 'unexplained/' .. n.id, false)
    end
  end
  return roots
end

-- Emits items depth first: the open part of the tree, or all of it while searching.
local function finder(state)
  return function(_, ctx)
    local searching = not ctx.filter:is_empty()
    ctx.picker.matcher.opts.keep_parents = searching
    local out, n = {}, 0
    local function emit(list)
      for i, it in ipairs(list) do
        n = n + 1
        it.sort = string.format('%05d', n)
        it.last = i == #list
        it.dir = #it.children > 0
        it.open = open_state[it.key] ~= false
        table.insert(out, it)
        if it.dir and (it.open or searching) then
          emit(it.children)
        end
      end
    end
    emit(build(state))
    return out
  end
end

local function format(item, picker)
  local ret = item.parent and Snacks.picker.format.tree(item, picker) or {}
  local function add(text, hl, virtual)
    table.insert(ret, { text, hl, virtual = virtual })
  end
  if item.kind == 'section' or item.kind == 'lane' then
    local icons = picker.opts.icons.files
    add(item.open and icons.dir_open or icons.dir, 'SnacksPickerDirectory', true)
    add(item.label, 'SnacksPickerDirectory')
  elseif item.kind == 'criterion' then
    local c = item.criterion
    local icon, hl = criterion_status(c)
    add(icon .. ' ', hl, true)
    add(c.id .. ' ', hl)
    add(c.text)
    local note = c.status ~= 'implemented' and c.status or (#c.tests == 0 and 'untested' or nil)
    if note then
      add(' (' .. note .. ')', 'SnacksPickerComment')
    end
  elseif item.kind == 'node' then
    local n = item.node
    add(string.format('%2d ', n.order), 'Number')
    add(n.risk == 'read' and '● ' or '○ ', n.risk == 'read' and 'Special' or 'NonText')
    local first = n.sites[1] or n.renames[1] or n.deletions[1]
    local icon, hl = file_icon(first and (first.file or first.to) or '')
    if icon then
      add(icon .. ' ', hl, true)
    end
    add(n.symbol, 'SnacksPickerFile')
    add('  ' .. n.caption, 'SnacksPickerComment')
  elseif item.kind == 'site' then
    local icon, hl = file_icon(item.site.file)
    if icon then
      add(icon .. ' ', hl, true)
    end
    local dir, base = item.site.file:match('([^/]*/)([^/]+)$')
    add(dir or '', 'SnacksPickerDir')
    add(base or item.site.file, 'SnacksPickerFile')
    add(':' .. item.site.start, 'LineNr')
  elseif item.kind == 'rename' then
    add('→ ', 'SnacksPickerComment', true)
    add('moved from ', 'SnacksPickerComment')
    add(item.rename.from, 'SnacksPickerDir')
  elseif item.kind == 'deletion' then
    add('✗ ', status_hl.missing, true)
    add('deleted ', 'SnacksPickerComment')
    add(item.deletion.file, 'SnacksPickerDir')
  elseif item.kind == 'test' then
    add('✓ ', status_hl.ok, true)
    add('test ', 'SnacksPickerComment')
    add(item.test.name, 'SnacksPickerFile')
    add('  ' .. item.test.file:match('[^/]+$') .. ':' .. item.test.line, 'SnacksPickerDir')
  elseif item.kind == 'edge' then
    add('→ ' .. item.edge.kind .. ' ', 'SnacksPickerComment')
    add(item.to.symbol, 'SnacksPickerFile')
  end
  return ret
end

local function confirm(state)
  return function(picker, item, action)
    if not item then
      return
    end
    if not item.file then
      if item.dir then
        open_state[item.key] = not item.open
        picker.list:set_target()
        picker:find({ refresh = true })
      end
      return
    end
    Snacks.picker.actions.jump(picker, item, action)
    local n = item.node or item.to
    if n and state.by_id[n.id] then
      vim.fn.setqflist({}, 'a', { idx = n.order })
    end
  end
end

local function close_parent(picker, item)
  if not item then
    return
  end
  local target = (item.dir and item.open) and item or item.parent
  if target then
    open_state[target.key] = false
    picker:find({ refresh = true })
  end
end

function P.open(state, opts)
  opts = opts or {}
  P.searching = false
  return Snacks.picker.pick({
    title = 'Change map',
    finder = finder(state),
    format = format,
    tree = true,
    focus = opts.focus or 'list',
    auto_close = false,
    jump = { close = false },
    layout = { preset = 'sidebar', preview = false },
    -- Snacks.win:equalize() treats every left split as stacked and halves its height; with the
    -- explorer beside this panel that shrinks the whole row and grows cmdheight. Opt out of it.
    -- ponytail: relies on snacks internals (vim.w.snacks_win.position), drop once snacks fixes equalize.
    on_show = function(picker)
      local win = picker.layout.root.win
      vim.w[win].snacks_win = vim.tbl_extend('force', vim.w[win].snacks_win or {}, { position = 'change-map' })
    end,
    sort = { fields = { 'sort' } },
    matcher = { sort_empty = false, fuzzy = false },
    filter = {
      -- Re-run the finder when the pattern toggles between empty and non-empty, like the explorer.
      transform = function(_, filter)
        local s = not filter:is_empty()
        if s ~= P.searching then
          P.searching = s
          return true
        end
      end,
    },
    confirm = confirm(state),
    actions = { change_map_close = close_parent },
    win = {
      list = {
        keys = {
          ['l'] = 'confirm',
          ['h'] = 'change_map_close',
          ['P'] = 'toggle_preview',
        },
      },
    },
  })
end

return P
