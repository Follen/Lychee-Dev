local function original() return 1 end
local function replacement() return 2 end
local first = original
original = replacement
local alias = first
first = replacement
alias()
