vim.bo.commentstring = "// %s"
vim.bo.shiftwidth = 2
vim.bo.tabstop = 2
vim.bo.expandtab = true
vim.bo.suffixesadd = ".lis"

pcall(vim.treesitter.start, 0, "lisette")
