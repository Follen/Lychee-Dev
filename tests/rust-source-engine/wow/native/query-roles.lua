local function Original(v) return v end
local function Replacement(v) return "replacement" end
local alias = Original
local saved = alias
alias = Replacement
local functionValueRead = saved
local firstCall = saved(1)
local secondCall = alias("two")
do
    local saved = Replacement
    local shadowCall = saved(3)
end
local object = { run = Original }
local other = { run = Replacement }
local memberSaved = object.run
object.run = Replacement
local savedMemberCall = memberSaved(4)
local currentMemberCall = object.run(5)
local otherMemberCall = other.run(6)
local unknownKey = ...
local dynamicMember = object[unknownKey]
-- saved(99), Original(99), object.run(99)
local text = "saved(99) Original(99) object.run(99)"
