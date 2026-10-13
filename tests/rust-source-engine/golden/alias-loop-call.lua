local function original() return 1 end
local function replacement() return 2 end
local alias = original
for index = 1, 2 do
    alias()
    alias = replacement
end
