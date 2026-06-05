local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h")
local parser_src = vim.fn.fnamemodify(plugin_dir, ":h") .. "/tree-sitter-lisette/src"
local parser_so = plugin_dir .. "/parser/lisette.so"

local function parser_is_stale()
  if vim.fn.isdirectory(parser_src) == 0 then
    return false
  end
  local so_time = vim.fn.getftime(parser_so)
  if so_time == -1 then
    return true
  end
  local src_time = math.max(
    vim.fn.getftime(parser_src .. "/parser.c"),
    vim.fn.getftime(parser_src .. "/scanner.c")
  )
  return src_time > so_time
end

if parser_is_stale() then
  vim.fn.mkdir(plugin_dir .. "/parser", "p")
  local result = vim.fn.system({
    "cc", "-o", parser_so, "-I", parser_src,
    parser_src .. "/parser.c", parser_src .. "/scanner.c",
    "-shared", "-Os", "-fPIC",
  })
  if vim.v.shell_error ~= 0 then
    vim.notify("Failed to compile Lisette tree-sitter parser:\n" .. result, vim.log.levels.WARN)
  end
end

if vim.fn.filereadable(parser_so) == 1 then
  vim.treesitter.language.add("lisette", { path = parser_so })
end

vim.lsp.enable("lisette")
