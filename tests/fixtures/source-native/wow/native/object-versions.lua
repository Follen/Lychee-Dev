---@param obj { value: number }
---@return number
function NeedNumber(obj)
    return 0
end

local Source = { value = 1, run = Identity }
local sourceOldValue = Source.value
local sourceOldRun = Source.run
local sourceOldAdded = Source.added
local oldGeneric = Identity(Source)
local good = NeedNumber(Source)
local Copied = CreateFromMixins(Source)

Source.value = "bad"
Source.run = Replacement
Source["added"] = true
local sourceNewValue = Source.value
local sourceNewRun = Source.run
local sourceNewAdded = Source.added
local newGeneric = Identity(Source)
local bad = NeedNumber(Source)
local copiedOldValue = Copied.value
local copiedOldRun = Copied.run
local copiedOldAdded = Copied.added
local stillGood = NeedNumber(Copied)

Mixin(Copied, Source)
local copiedNewValue = Copied.value
local copiedNewRun = Copied.run
local copiedNewAdded = Copied.added
local copiedBad = NeedNumber(Copied)
local copiedGeneric = Identity(Copied)

local Twin = { value = "bad", run = Replacement, added = true }
local twinValue = Twin.value
local twinRun = Twin.run
local twinAdded = Twin.added

local key = ...
Source[key] = false
local dynamicValue = Source.value
local dynamicRun = Source.run
local dynamicAdded = Source.added
local dynamic = NeedNumber(Source)
local dynamicGeneric = Identity(Source)
