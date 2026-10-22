local Child = { value = 1 }
local Container = { child = Child }
local Alias = Container
local Copy = CreateFromMixins(Container)
local nestedBefore = Copy
Child.value = "bad"
local nestedAfter = Copy
Child.self = Child
local cycleView = Child

local NilSource = { value = 1 }
local NilTarget = { value = "keep" }
NilSource.value = nil
Mixin(NilTarget, NilSource)
local deletedSource = NilSource
local keptTarget = NilTarget

local Undelivered = { value = 7 }
local Escaped = { value = 8 }
GlobalEscape = Escaped
local unknown = ...
unknown(Child)
local unknownChild = Child
local unknownNested = Copy
local stableLocal = Undelivered
local escapedUnknown = Escaped

local Captured = { value = 9 }
local function BadCaptured()
    Captured.value = "bad"
    UnknownEffect()
end
BadCaptured()
local capturedUnknown = Captured
local stillStable = Undelivered
local containerAlias = Alias
