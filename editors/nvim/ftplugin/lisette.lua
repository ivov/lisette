vim.bo.commentstring = "// %s"
vim.bo.shiftwidth = 2
vim.bo.tabstop = 2
vim.bo.expandtab = true
vim.bo.suffixesadd = ".lis"

local ok, err = pcall(vim.treesitter.start, 0, "lisette")
if not ok then
  vim.notify("Lisette: failed to start syntax highlighting:\n" .. tostring(err), vim.log.levels.WARN)
end
