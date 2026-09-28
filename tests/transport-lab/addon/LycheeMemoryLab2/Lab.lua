-- Isolated transport lab. No hooks, SavedVariables, network, protected actions,
-- or calls into Lychee Dev. The retained snapshots intentionally mimic tickets.
local HEAD = "LYCHEE_MEMORY_" .. "LAB2_HEAD|"
local BODY = "LYCHEE_MEMORY_" .. "LAB2_BODY|"
local ids = { "REQ-LAB2-A", "REQ-LAB2-B", "REQ-LAB2-C", "REQ-LAB2-D", "REQ-LAB2-E" }
local plans = {
    { "loaded", "running", "reported", "none", "none" },
    { "running", "reported", "reported", "none", "none" },
    { "reported", "acknowledged", "reported", "none", "none" },
    { "acknowledged", "acknowledged", "acknowledged", "loaded", "running" },
    { "acknowledged", "acknowledged", "acknowledged", "reported", "reported" },
    { "acknowledged", "acknowledged", "acknowledged", "acknowledged", "acknowledged" },
}
local sizes = { 1024, 65536, 131072, 524288, 4096 }
local kinds = { "ascii", "utf8", "binary", "binary", "duplicate" }
local born = { 2, 1, 0, 4, 4 }
local run, epoch, current, timer, retained

local function adler32(bytes)
    local a, b = 1, 0
    for i = 1, #bytes do
        a = (a + string.byte(bytes, i)) % 65521
        b = (b + a) % 65521
    end
    return string.format("%08x", b * 65536 + a)
end

local function payload(index, generation)
    local size = sizes[index]
    local kind = kinds[index]
    local block
    if kind == "ascii" then
        block = "probeStatus=completed;result=hello;ticket=A;generation=" .. generation .. ";"
    elseif kind == "utf8" then
        block = "探针结果：法术、目标、秘密值边界；generation=" .. generation .. ";"
    elseif kind == "duplicate" then
        block = "same-content-across-generations;"
    else
        local bytes = {}
        for i = 0, 255 do
            bytes[#bytes + 1] = string.char((i * 73 + index * 19 + generation * 11) % 256)
        end
        block = table.concat(bytes)
    end
    local repeats = math.ceil(size / #block)
    return string.sub(string.rep(block, repeats), 1, size), kind
end

local function snapshot(generation)
    local states = plans[generation + 1]
    if not states then return nil, "end of lifecycle" end
    local reports, rows = {}, {}
    for index, id in ipairs(ids) do
        local state = states[index]
        local bytes, digest, kind = 0, "00000000", kinds[index]
        local reportEpoch = "-"
        if state == "reported" then
            local content = payload(index, born[index])
            bytes, digest = #content, adler32(content)
            if not retained[id] then
                retained[id] = BODY .. run .. "|" .. id .. "|" .. born[index] .. "|"
                .. kind .. "|" .. bytes .. "|" .. digest .. "|" .. content
                .. "|END|" .. run .. "|" .. id .. "|" .. born[index]
            end
            reports[id] = retained[id]
            reportEpoch = born[index]
        elseif state == "acknowledged" then
            retained[id] = nil
        end
        rows[#rows + 1] = table.concat({ id, state, kind, bytes, digest, reportEpoch }, ",")
    end
    local head = HEAD .. run .. "|" .. generation .. "|" .. table.concat(rows, ";")
        .. "|END|" .. run .. "|" .. generation
    -- One published table replaces the prior snapshot. Readers must still
    -- guard against stale strings left behind in the Lua allocator.
    current = { run = run, epoch = generation, head = head, reports = reports }
    _G.LycheeMemoryLab2 = current
    epoch = generation
    print("LycheeMemoryLab2: run=" .. run .. " epoch=" .. generation)
    return true
end

local function stopCycle()
    if timer and timer.Cancel then timer:Cancel() end
    timer = nil
end

local function reset()
    stopCycle()
    local seconds = (GetServerTime and GetServerTime()) or time()
    local ticks = math.floor(((GetTime and GetTime()) or 0) * 1000) % 4294967296
    run = string.format("%08x%08x", seconds, ticks)
    retained = {}
    return snapshot(0)
end

SLASH_LYCHEEMEMORYLABTWO1 = "/memlab2"
SlashCmdList.LYCHEEMEMORYLABTWO = function(input)
    input = string.match(input or "", "^%s*(.-)%s*$")
    if input == "next" then
        stopCycle()
        local ok, reason = snapshot(epoch + 1)
        if not ok then print("LycheeMemoryLab2: " .. reason) end
    elseif input == "cycle" then
        stopCycle()
        print("LycheeMemoryLab2: cycling every 3 seconds")
        local function advance()
            if epoch >= #plans - 1 then timer = nil; return end
            snapshot(epoch + 1)
            if epoch < #plans - 1 and C_Timer and C_Timer.NewTimer then
                timer = C_Timer.NewTimer(3, advance)
            else
                timer = nil
            end
        end
        if C_Timer and C_Timer.NewTimer then timer = C_Timer.NewTimer(3, advance)
        else print("LycheeMemoryLab2: timer unavailable; use /memlab2 next") end
    elseif input == "reset" then
        reset()
    elseif input == "status" or input == "" then
        print("LycheeMemoryLab2: run=" .. tostring(run) .. " epoch=" .. tostring(epoch))
    elseif input == "clear" then
        stopCycle()
        current = nil
        _G.LycheeMemoryLab2 = nil
        print("LycheeMemoryLab2: released")
    else
        print("LycheeMemoryLab2: use status | next | cycle | reset | clear")
    end
end

local loader = CreateFrame("Frame")
loader:RegisterEvent("PLAYER_LOGIN")
loader:SetScript("OnEvent", function(self)
    self:UnregisterAllEvents()
    self:SetScript("OnEvent", nil)
    local ok, reason = pcall(reset)
    if not ok then print("LycheeMemoryLab2: initialization failed: " .. tostring(reason)) end
end)
