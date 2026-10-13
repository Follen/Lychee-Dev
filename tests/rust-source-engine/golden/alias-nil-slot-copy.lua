local function original() return 1 end
local unused, padded = original
local alias = padded
alias()
