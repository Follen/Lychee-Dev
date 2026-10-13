local function original() return 1 end
local function replacement() return 2 end
local before = original
local alias = original
alias()
alias = replacement
alias()
return before
