local function original() return 1 end
local alias = original
function alias() return 2 end
alias()
