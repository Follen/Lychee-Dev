local numberResult = Identity(7)
local stringResult = Identity("seven")
local Source = {}
Source.echo = Identity
local Copied = CreateFromMixins(Source)
Source.echo = Replacement
local copiedNumber = Copied.echo(7)
local replacedString = Source.echo("seven")
local colonNumber = Methods:Echo("label", 7)
local dotString = Methods.Echo(Methods, "label", "seven")
local wrongLabel = Methods:Echo(99, 7)

---@return string
function WrongCopiedReturn()
    return Copied.echo(7)
end

---@return string
function WrongMethodReturn()
    return Methods:Echo("label", 7)
end

---@return number
function WrongLateReturn()
    return Identity(7)
end
