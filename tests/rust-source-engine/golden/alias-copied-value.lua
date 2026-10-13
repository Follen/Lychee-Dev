local function original() return 1 end
local function replacement() return 2 end
local alias = original
original = replacement
alias()
