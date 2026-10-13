local function original() return 1 end
local function replacement() return 2 end
local alias = original
local function invoke() alias() end
alias = replacement
invoke()
