A = {}
A.run = F1
local x = CreateFromMixins(A)
local copied = x.run
A.run = F2
local first = x.run(7)
local wrongFirst = x.run("bad")
Mixin(x, A)
local second = x.run("ok")
local wrongSecond = x.run(7)

---@return string
local function WrongReturn()
    return copied(7)
end

return first, second
