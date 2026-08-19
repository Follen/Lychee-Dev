local ADDON_NAME, ns = ...

local performance = {}
local MAX_SESSIONS = 12
local MAX_SAMPLES = 720
local MAX_OBJECTS = 1600
local MAX_GRAPH_NODES = 5000
local MAX_GRAPH_DEPTH = 8
local MAX_FUNCTIONS = 800
local MAX_GLOBAL_ROOTS = 16
local MAX_GLOBAL_SCAN = 12000
local MAX_FRAME_SCAN = 5000
local SAMPLE_INTERVAL = 0.25
local OBJECT_SAMPLE_INTERVAL = 1

local SCRIPT_TYPES = {
    "OnEvent", "OnUpdate", "OnShow", "OnHide", "OnClick", "OnEnter", "OnLeave",
    "OnValueChanged", "OnSizeChanged", "OnAttributeChanged", "OnMouseWheel",
    "OnDragStart", "OnDragStop",
}

local METRICS = {
    { key = "sessionAverage", enum = "SessionAverageTime" },
    { key = "recentAverage", enum = "RecentAverageTime" },
    { key = "encounterAverage", enum = "EncounterAverageTime" },
    { key = "lastTime", enum = "LastTime" },
    { key = "peakTime", enum = "PeakTime" },
    { key = "over1", enum = "CountTimeOver1Ms" },
    { key = "over5", enum = "CountTimeOver5Ms" },
    { key = "over10", enum = "CountTimeOver10Ms" },
    { key = "over50", enum = "CountTimeOver50Ms" },
    { key = "over100", enum = "CountTimeOver100Ms" },
    { key = "over500", enum = "CountTimeOver500Ms" },
    { key = "over1000", enum = "CountTimeOver1000Ms" },
}

local sessions = {}
local activeSession
local lastBenchmark

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function SafeNumber(value)
    if value == nil or IsSecret(value) then return nil end
    return tonumber(value)
end

local function ReadCpuBound()
    if not IsCpuBound then return nil end
    local succeeded, value = pcall(IsCpuBound)
    if not succeeded or IsSecret(value) then return nil end
    return value
end

local function SafeCall(object, methodName, ...)
    if object == nil or IsSecret(object) then return nil end
    local methodSucceeded, method = pcall(function() return object[methodName] end)
    if not methodSucceeded or type(method) ~= "function" or IsSecret(method) then return nil end
    local succeeded, result = pcall(method, object, ...)
    if not succeeded or IsSecret(result) then return nil end
    return result
end

local function SafeResults(object, methodName)
    if object == nil or IsSecret(object) then return {} end
    local methodSucceeded, method = pcall(function() return object[methodName] end)
    if not methodSucceeded or type(method) ~= "function" or IsSecret(method) then return {} end
    local packed = { pcall(method, object) }
    if not packed[1] then return {} end
    local results = {}
    for index = 2, #packed do
        if packed[index] ~= nil and not IsSecret(packed[index]) then
            results[#results + 1] = packed[index]
        end
    end
    return results
end

local function GetMetricEnum(enumName)
    local enumTable = Enum and Enum.AddOnProfilerMetric
    local value = enumTable and enumTable[enumName]
    return IsSecret(value) and nil or value
end

local function ReadProfilerMetric(reader, addonName, enumName)
    local metric = GetMetricEnum(enumName)
    if not reader or metric == nil then return nil end
    local succeeded, value
    if addonName then
        succeeded, value = pcall(reader, addonName, metric)
    else
        succeeded, value = pcall(reader, metric)
    end
    return succeeded and SafeNumber(value) or nil
end

local function ReadMetricSet(addonName)
    local profiler = C_AddOnProfiler
    local result = {}
    for index = 1, #METRICS do
        local definition = METRICS[index]
        result[definition.key] = ReadProfilerMetric(
            profiler and profiler.GetAddOnMetric, addonName, definition.enum)
    end
    return result
end

local function ReadOverallMetricSet(reader)
    local result = {}
    for index = 1, #METRICS do
        local definition = METRICS[index]
        result[definition.key] = ReadProfilerMetric(reader, nil, definition.enum)
    end
    return result
end

local function ReadMemory(addonName, refresh)
    if refresh and UpdateAddOnMemoryUsage then
        pcall(UpdateAddOnMemoryUsage)
    end
    if not GetAddOnMemoryUsage then return nil end
    local succeeded, value = pcall(GetAddOnMemoryUsage, addonName)
    return succeeded and SafeNumber(value) or nil
end

local function IsLoaded(addonName)
    local succeeded, loaded = pcall(ns.Client.IsAddOnLoaded, addonName)
    if not succeeded or IsSecret(loaded) then return false end
    return loaded and true or false
end

local function RelativeAddonPercent(addonValue, overallValue, applicationValue)
    addonValue = SafeNumber(addonValue)
    overallValue = SafeNumber(overallValue)
    applicationValue = SafeNumber(applicationValue)
    if not addonValue or not overallValue or not applicationValue then return nil end
    local relativeTotal = applicationValue - overallValue + addonValue
    if relativeTotal <= 0 then return nil end
    return math.max(0, addonValue / relativeTotal * 100)
end

local function OverallPercent(overallValue, applicationValue)
    overallValue = SafeNumber(overallValue)
    applicationValue = SafeNumber(applicationValue)
    if not overallValue or not applicationValue or applicationValue <= 0 then return nil end
    return math.max(0, overallValue / applicationValue * 100)
end

local function BuildFindings(entry)
    local metrics = entry.metrics
    local findings = {}
    local severity = 0
    local recent = metrics.recentAverage or 0
    local function Add(level, code, value)
        findings[#findings + 1] = { level = level, code = code, value = value }
        severity = math.max(severity, level)
    end

    if (metrics.over1000 or 0) > 0 then
        Add(recent >= 2 and 4 or 2, "freeze_1000", metrics.over1000)
    elseif (metrics.over500 or 0) > 0 then
        Add(recent >= 2 and 3 or 2, "freeze_500", metrics.over500)
    end
    if (metrics.over100 or 0) > 0 then Add(recent >= 2 and 3 or 1, "spike_100", metrics.over100) end
    if (metrics.over50 or 0) > 0 then Add(recent >= 2 and 2 or 1, "spike_50", metrics.over50) end
    if recent >= 5 then
        Add(3, "sustained_high", metrics.recentAverage)
    elseif recent >= 2 then
        Add(2, "sustained_medium", metrics.recentAverage)
    end
    if (metrics.encounterAverage or 0) >= 5
        and (metrics.encounterAverage or 0) >= math.max(1, (metrics.sessionAverage or 0) * 1.8) then
        Add(3, "encounter_hot", metrics.encounterAverage)
    end
    if (metrics.peakTime or 0) >= 50 and (metrics.recentAverage or 0) < 2 then
        Add(1, "bursty", metrics.peakTime)
    end
    entry.severity = severity
    entry.findings = findings
end

function performance.CollectHealth()
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local succeeded, enabled = pcall(ns.Client.HasAddOnProfiler)
    local profilerEnabled = succeeded and not IsSecret(enabled) and enabled and true or false
    local overall = ReadOverallMetricSet(C_AddOnProfiler and C_AddOnProfiler.GetOverallMetric)
    local application = ReadOverallMetricSet(C_AddOnProfiler and C_AddOnProfiler.GetApplicationMetric)
    if UpdateAddOnMemoryUsage then pcall(UpdateAddOnMemoryUsage) end

    local addons = {}
    local count = ns.Client.GetNumAddOns()
    for index = 1, count do
        local name, title = ns.Client.GetAddOnInfo(index)
        if type(name) == "string" and not IsSecret(name) and IsLoaded(name) then
            local entry = {
                name = name,
                title = type(title) == "string" and not IsSecret(title) and title or name,
                metrics = ReadMetricSet(name),
                memory = ReadMemory(name, false),
            }
            entry.addonShare = RelativeAddonPercent(entry.metrics.recentAverage,
                overall.recentAverage, application.recentAverage)
            entry.overallShare = OverallPercent(overall.recentAverage, application.recentAverage)
            BuildFindings(entry)
            addons[#addons + 1] = entry
        end
    end
    table.sort(addons, function(left, right)
        if left.severity ~= right.severity then return left.severity > right.severity end
        local leftRecent = left.metrics.recentAverage or -1
        local rightRecent = right.metrics.recentAverage or -1
        if leftRecent ~= rightRecent then return leftRecent > rightRecent end
        return left.name < right.name
    end)
    return true, {
        kind = "health",
        generatedAt = time(),
        profilerEnabled = profilerEnabled,
        scriptProfile = GetCVarBool and GetCVarBool("scriptProfile") and true or false,
        cpuBound = ReadCpuBound(),
        addons = addons,
        overall = overall,
        application = application,
    }
end

local function ParseSource(location)
    if type(location) ~= "string" or location == "" then return "unknown", location end
    local normalized = location:gsub("/", "\\")
    local addon, file, line = normalized:match("[Ii]nterface\\[Aa]dd[Oo]ns\\([^\\]+)\\(.+):(%d+)")
    if addon then return addon, file .. ":" .. line end
    if normalized:find("[Bb]lizzard") or normalized:find("[Ff]rame[Xx][Mm][Ll]") then
        return "Blizzard", normalized
    end
    return "unknown", normalized
end

local function IsUIObject(value)
    if value == nil or IsSecret(value) then return false end
    local succeeded, method = pcall(function() return value.GetObjectType end)
    return succeeded and type(method) == "function" and not IsSecret(method)
end

local function ReadFrameCPU(frame, includeChildren)
    if not GetFrameCPUUsage or not (GetCVarBool and GetCVarBool("scriptProfile")) then return nil, nil end
    local succeeded, elapsed, calls = pcall(GetFrameCPUUsage, frame, includeChildren and true or false)
    if not succeeded or IsSecret(elapsed) or IsSecret(calls) then return nil, nil end
    return tonumber(elapsed), tonumber(calls)
end

local function ReadFunctionCPU(func, includeSubroutines)
    if type(func) ~= "function" or not GetFunctionCPUUsage
        or not (GetCVarBool and GetCVarBool("scriptProfile")) then return nil, nil end
    local succeeded, elapsed, calls = pcall(GetFunctionCPUUsage, func,
        includeSubroutines and true or false)
    if not succeeded or IsSecret(elapsed) or IsSecret(calls) then return nil, nil end
    return tonumber(elapsed), tonumber(calls)
end

local function HasOnUpdate(object)
    if SafeCall(object, "HasScript", "OnUpdate") == false then return false end
    local script = SafeCall(object, "GetScript", "OnUpdate")
    return type(script) == "function" and not IsSecret(script)
end

local function AddCount(counts, key, amount)
    key = type(key) == "string" and key or "unknown"
    counts[key] = (counts[key] or 0) + (amount or 1)
end

local function ReadPool(tableValue, path)
    if type(tableValue) ~= "table" then return nil end
    local metatable = getmetatable(tableValue)
    local indexTable = type(metatable) == "table" and rawget(metatable, "__index") or nil
    local activeMethod = rawget(tableValue, "GetNumActive")
        or (type(indexTable) == "table" and rawget(indexTable, "GetNumActive"))
    if type(activeMethod) ~= "function" then return nil end
    local succeeded, active = pcall(activeMethod, tableValue)
    if not succeeded or IsSecret(active) then return nil end
    active = tonumber(active)
    if not active then return nil end
    local inactive = rawget(tableValue, "inactiveObjects")
    local inactiveCount = type(inactive) == "table" and #inactive or nil
    return {
        identity = tableValue,
        path = path,
        active = active,
        inactive = inactiveCount,
        capacity = inactiveCount and (active + inactiveCount) or nil,
    }
end

local function SnapshotTarget(target, label, addonName)
    local snapshot = {
        label = label,
        objects = {},
        count = 0,
        visible = 0,
        onUpdate = 0,
        byType = {},
        bySource = {},
        pools = {},
        functions = {},
        functionPaths = {},
        functionCount = 0,
        functionReferences = 0,
        frameScanAvailable = type(EnumerateFrames) == "function",
        framesScanned = 0,
        attributedFrames = 0,
        graph = { tables = 0, functions = 0, strings = 0, entries = 0, truncated = false },
    }
    local seenObjects, seenGraph = {}, {}

    local function RegisterFunction(func, path, kind, ownerPath)
        if type(func) ~= "function" or IsSecret(func) then return end
        snapshot.functionReferences = snapshot.functionReferences + 1
        snapshot.functionPaths[path] = func
        local record = snapshot.functions[func]
        if record then
            record.references = record.references + 1
            if #record.paths < 6 then record.paths[#record.paths + 1] = path end
            return
        end
        if snapshot.functionCount >= MAX_FUNCTIONS then
            snapshot.graph.truncated = true
            return
        end
        local selfTime, calls = ReadFunctionCPU(func, false)
        local inclusiveTime = ReadFunctionCPU(func, true)
        snapshot.functionCount = snapshot.functionCount + 1
        snapshot.functions[func] = {
            identity = func,
            path = path,
            paths = { path },
            references = 1,
            kind = kind or "namespace",
            ownerPath = ownerPath,
            selfTime = selfTime,
            inclusiveTime = inclusiveTime,
            calls = calls,
        }
    end

    local function VisitUI(object, path, depth)
        if snapshot.count >= MAX_OBJECTS or depth > MAX_GRAPH_DEPTH
            or seenObjects[object] or not IsUIObject(object) then return end
        seenObjects[object] = true
        snapshot.count = snapshot.count + 1
        local objectType = SafeCall(object, "GetObjectType") or type(object)
        local name = SafeCall(object, "GetDebugName") or SafeCall(object, "GetName") or path
        local shown = SafeCall(object, "IsShown") and true or false
        local location = SafeCall(object, "GetSourceLocation")
        local source, sourceFile = ParseSource(location)
        local selfTime, selfCalls = ReadFrameCPU(object, false)
        local treeTime, treeCalls = ReadFrameCPU(object, true)
        local onUpdate = HasOnUpdate(object)
        local record = {
            identity = object,
            path = path,
            name = name,
            objectType = objectType,
            shown = shown,
            source = source,
            sourceFile = sourceFile,
            depth = depth,
            onUpdate = onUpdate,
            selfTime = selfTime,
            selfCalls = selfCalls,
            treeTime = treeTime,
            treeCalls = treeCalls,
        }
        snapshot.objects[object] = record
        if shown then snapshot.visible = snapshot.visible + 1 end
        if onUpdate then snapshot.onUpdate = snapshot.onUpdate + 1 end
        AddCount(snapshot.byType, objectType)
        AddCount(snapshot.bySource, source)

        for index = 1, #SCRIPT_TYPES do
            local scriptType = SCRIPT_TYPES[index]
            local supported = SafeCall(object, "HasScript", scriptType)
            if supported then
                local script = SafeCall(object, "GetScript", scriptType)
                RegisterFunction(script, path .. "." .. scriptType, "frame_script", path)
            end
        end

        local children = SafeResults(object, "GetChildren")
        for index = 1, #children do
            VisitUI(children[index], path .. ".children[" .. index .. "]", depth + 1)
        end
        local regions = SafeResults(object, "GetRegions")
        for index = 1, #regions do
            VisitUI(regions[index], path .. ".regions[" .. index .. "]", depth + 1)
        end
    end

    local function VisitGraph(value, path, depth)
        if depth > MAX_GRAPH_DEPTH or snapshot.graph.entries >= MAX_GRAPH_NODES or IsSecret(value) then
            snapshot.graph.truncated = snapshot.graph.entries >= MAX_GRAPH_NODES
            return
        end
        local valueType = type(value)
        if valueType == "table" then
            if seenGraph[value] then return end
            seenGraph[value] = true
            snapshot.graph.tables = snapshot.graph.tables + 1
            local pool = ReadPool(value, path)
            if pool then snapshot.pools[value] = pool end
            local cursor
            while snapshot.graph.entries < MAX_GRAPH_NODES do
                local succeeded, key, child = pcall(next, value, cursor)
                if not succeeded or key == nil then break end
                cursor = key
                snapshot.graph.entries = snapshot.graph.entries + 1
                local childPath = path .. ".<secret-key>"
                if not IsSecret(key) then
                    local textOk, keyText = pcall(tostring, key)
                    if textOk then childPath = path .. "." .. keyText end
                end
                if IsUIObject(child) then VisitUI(child, childPath, 0) end
                VisitGraph(child, childPath, depth + 1)
            end
        elseif valueType == "function" then
            snapshot.graph.functions = snapshot.graph.functions + 1
            RegisterFunction(value, path, "namespace")
        elseif valueType == "string" then
            snapshot.graph.strings = snapshot.graph.strings + 1
        elseif IsUIObject(value) then
            VisitUI(value, path, 0)
        end
    end

    if IsUIObject(target) then
        VisitUI(target, label or "object", 0)
    elseif type(target) == "table" then
        VisitGraph(target, label or "table", 0)
    end

    if addonName and snapshot.frameScanAvailable then
        local current
        while snapshot.framesScanned < MAX_FRAME_SCAN do
            local succeeded, frame = pcall(EnumerateFrames, current)
            if not succeeded or not frame or frame == current then break end
            current = frame
            snapshot.framesScanned = snapshot.framesScanned + 1
            local location = SafeCall(frame, "GetSourceLocation")
            local source = ParseSource(location)
            if source == addonName then
                snapshot.attributedFrames = snapshot.attributedFrames + 1
                local name = SafeCall(frame, "GetDebugName") or SafeCall(frame, "GetName")
                    or ("frame[" .. snapshot.attributedFrames .. "]")
                VisitUI(frame, "frames." .. tostring(name), 0)
            end
        end
        if snapshot.framesScanned >= MAX_FRAME_SCAN then snapshot.truncated = true end
    end
    snapshot.truncated = snapshot.truncated or snapshot.count >= MAX_OBJECTS or snapshot.graph.truncated
    return snapshot
end

local function UpdateObjectState(session, snapshot, initial)
    local state = session.objectState
    if not state then
        state = {
            known = {}, pools = {}, created = 0, activations = 0, reused = 0,
            hidden = 0, peak = 0, functions = {}, pathFunctions = {},
            functionChurn = {}, newFunctions = 0, functionReplacements = 0,
            peakFunctions = 0, peakFunctionReferences = 0,
        }
        session.objectState = state
    end
    state.peak = math.max(state.peak, snapshot.count)
    for object, record in pairs(snapshot.objects) do
        local known = state.known[object]
        if not known then
            known = { lastShown = record.shown, everShown = record.shown, first = record }
            state.known[object] = known
            if not initial then state.created = state.created + 1 end
        else
            if record.shown and not known.lastShown then
                state.activations = state.activations + 1
                if known.everShown then state.reused = state.reused + 1 end
                known.everShown = true
            elseif not record.shown and known.lastShown then
                state.hidden = state.hidden + 1
            end
            known.lastShown = record.shown
        end
    end
    for identity, pool in pairs(snapshot.pools) do
        local poolState = state.pools[identity]
        if not poolState then
            state.pools[identity] = {
                path = pool.path,
                initialActive = pool.active,
                initialCapacity = pool.capacity,
                lastActive = pool.active,
                peakActive = pool.active,
                acquisitions = 0,
                releases = 0,
                reuseAcquisitions = 0,
            }
        else
            local delta = pool.active - poolState.lastActive
            if delta > 0 then
                poolState.acquisitions = poolState.acquisitions + delta
                if not pool.capacity or not poolState.initialCapacity
                    or pool.capacity <= poolState.initialCapacity then
                    poolState.reuseAcquisitions = poolState.reuseAcquisitions + delta
                end
            elseif delta < 0 then
                poolState.releases = poolState.releases - delta
            end
            poolState.lastActive = pool.active
            poolState.peakActive = math.max(poolState.peakActive, pool.active)
            poolState.finalCapacity = pool.capacity
        end
    end
    state.peakFunctions = math.max(state.peakFunctions, snapshot.functionCount or 0)
    state.peakFunctionReferences = math.max(state.peakFunctionReferences,
        snapshot.functionReferences or 0)
    for func, record in pairs(snapshot.functions or {}) do
        local known = state.functions[func]
        if not known then
            known = { first = record, last = record }
            state.functions[func] = known
            if not initial then state.newFunctions = state.newFunctions + 1 end
        else
            known.last = record
        end
        for index = 1, #(record.paths or {}) do
            local path = record.paths[index]
            local previous = state.pathFunctions[path]
            if previous and previous ~= func then
                state.functionReplacements = state.functionReplacements + 1
                state.functionChurn[path] = (state.functionChurn[path] or 0) + 1
            end
            state.pathFunctions[path] = func
        end
    end
    session.objectLastSnapshot = snapshot
end

local function SerializeObjectSummary(session)
    local state = session.objectState
    local baseline = session.objectBaseline
    local final = session.objectLastSnapshot
    if not state or not baseline or not final then return nil end
    local summary = {
        label = final.label,
        initialObjects = baseline.count,
        finalObjects = final.count,
        peakObjects = state.peak,
        newlyObserved = state.created,
        activations = state.activations,
        reusedActivations = state.reused,
        hiddenTransitions = state.hidden,
        reuseRate = state.activations > 0 and state.reused / state.activations * 100 or nil,
        visible = final.visible,
        onUpdate = final.onUpdate,
        byType = final.byType,
        bySource = final.bySource,
        graph = final.graph,
        truncated = final.truncated,
        pools = {},
        hotFrames = {},
        hotFunctions = {},
        functionChurn = {},
        functionsDiscovered = state.peakFunctions,
        functionReferences = state.peakFunctionReferences,
        newFunctionInstances = state.newFunctions,
        functionReplacements = state.functionReplacements,
        rootsDiscovered = final.rootCount or 0,
        framesScanned = final.framesScanned or 0,
        attributedFrames = final.attributedFrames or 0,
        frameScanAvailable = final.frameScanAvailable,
    }
    for _, pool in pairs(state.pools) do
        summary.pools[#summary.pools + 1] = {
            path = pool.path,
            initialActive = pool.initialActive,
            peakActive = pool.peakActive,
            finalActive = pool.lastActive,
            initialCapacity = pool.initialCapacity,
            finalCapacity = pool.finalCapacity or pool.initialCapacity,
            acquisitions = pool.acquisitions,
            releases = pool.releases,
            reuseAcquisitions = pool.reuseAcquisitions,
            reuseRate = pool.acquisitions > 0 and pool.reuseAcquisitions / pool.acquisitions * 100 or nil,
        }
    end
    table.sort(summary.pools, function(left, right) return left.path < right.path end)

    for object, finalRecord in pairs(final.objects) do
        local initialRecord = baseline.objects[object]
        if initialRecord and finalRecord.selfTime and initialRecord.selfTime then
            local elapsed = math.max(0, finalRecord.selfTime - initialRecord.selfTime)
            local calls = math.max(0, (finalRecord.selfCalls or 0) - (initialRecord.selfCalls or 0))
            if elapsed > 0 or calls > 0 then
                summary.hotFrames[#summary.hotFrames + 1] = {
                    path = finalRecord.path,
                    name = finalRecord.name,
                    objectType = finalRecord.objectType,
                    source = finalRecord.source,
                    sourceFile = finalRecord.sourceFile,
                    elapsed = elapsed,
                    calls = calls,
                    average = calls > 0 and elapsed / calls or nil,
                    onUpdate = finalRecord.onUpdate,
                }
            end
        end
    end
    table.sort(summary.hotFrames, function(left, right) return left.elapsed > right.elapsed end)
    while #summary.hotFrames > 50 do summary.hotFrames[#summary.hotFrames] = nil end

    for _, known in pairs(state.functions) do
        local firstRecord, lastRecord = known.first, known.last
        if firstRecord.selfTime and lastRecord.selfTime then
            local elapsed = math.max(0, lastRecord.selfTime - firstRecord.selfTime)
            local inclusive = firstRecord.inclusiveTime and lastRecord.inclusiveTime
                and math.max(0, lastRecord.inclusiveTime - firstRecord.inclusiveTime) or nil
            local calls = math.max(0, (lastRecord.calls or 0) - (firstRecord.calls or 0))
            if elapsed > 0 or calls > 0 then
                summary.hotFunctions[#summary.hotFunctions + 1] = {
                    path = lastRecord.path,
                    kind = lastRecord.kind,
                    ownerPath = lastRecord.ownerPath,
                    elapsed = elapsed,
                    inclusive = inclusive,
                    calls = calls,
                    average = calls > 0 and elapsed / calls or nil,
                    references = lastRecord.references,
                }
            end
        end
    end
    table.sort(summary.hotFunctions, function(left, right)
        if left.elapsed ~= right.elapsed then return left.elapsed > right.elapsed end
        return left.calls > right.calls
    end)
    while #summary.hotFunctions > 80 do summary.hotFunctions[#summary.hotFunctions] = nil end
    for path, replacements in pairs(state.functionChurn) do
        summary.functionChurn[#summary.functionChurn + 1] = {
            path = path,
            replacements = replacements,
        }
    end
    table.sort(summary.functionChurn, function(left, right)
        if left.replacements ~= right.replacements then
            return left.replacements > right.replacements
        end
        return left.path < right.path
    end)
    return summary
end

local function SnapshotSavedVariable(value)
    local result = { bytes = 0, tables = 0, entries = 0, strings = 0, functions = 0, truncated = false }
    local seen = {}
    local function Visit(current, depth)
        if depth > MAX_GRAPH_DEPTH or result.entries >= MAX_GRAPH_NODES or IsSecret(current) then
            result.truncated = result.entries >= MAX_GRAPH_NODES
            return
        end
        local valueType = type(current)
        if valueType == "string" then
            result.strings = result.strings + 1
            result.bytes = result.bytes + #current
        elseif valueType == "table" then
            if seen[current] then return end
            seen[current] = true
            result.tables = result.tables + 1
            result.bytes = result.bytes + 48
            local cursor
            while result.entries < MAX_GRAPH_NODES do
                local succeeded, key, child = pcall(next, current, cursor)
                if not succeeded or key == nil then break end
                cursor = key
                result.entries = result.entries + 1
                Visit(key, depth + 1)
                Visit(child, depth + 1)
                result.bytes = result.bytes + 16
            end
        elseif valueType == "function" then
            result.functions = result.functions + 1
            result.bytes = result.bytes + 16
        else
            result.bytes = result.bytes + 8
        end
    end
    Visit(value, 0)
    return result
end

local function ParseVariableNames(text, output, seen)
    if type(text) ~= "string" or IsSecret(text) then return end
    for name in text:gmatch("[_%a][_%w]*") do
        if not seen[name] then
            seen[name] = true
            output[#output + 1] = name
        end
    end
end

local function GetSavedVariableNames(addonName)
    local names, seen = {}, {}
    local succeeded, account = pcall(ns.Client.GetAddOnMetadata, addonName, "SavedVariables")
    if succeeded then ParseVariableNames(account, names, seen) end
    succeeded, account = pcall(ns.Client.GetAddOnMetadata, addonName, "SavedVariablesPerCharacter")
    if succeeded then ParseVariableNames(account, names, seen) end
    return names
end

local function NormalizeName(value)
    if type(value) ~= "string" or IsSecret(value) then return "" end
    return value:lower():gsub("|c%x%x%x%x%x%x%x%x", ""):gsub("|r", ""):gsub("[^%w]", "")
end

local function IsAddonRootName(key, addonName)
    local normalizedKey = NormalizeName(key)
    local normalizedAddon = NormalizeName(addonName)
    if #normalizedKey < 4 or #normalizedAddon < 4 then return false end
    return normalizedKey == normalizedAddon
        or normalizedKey:find(normalizedAddon, 1, true) ~= nil
        or normalizedAddon:find(normalizedKey, 1, true) ~= nil
end

local function BuildAddonRoots(addonName, extraTarget, extraLabel)
    local roots, paths, count = {}, {}, 0
    local function Add(path, value)
        if count >= MAX_GLOBAL_ROOTS or paths[value] or IsSecret(value) then return end
        local valueType = type(value)
        if valueType ~= "table" and valueType ~= "function" and not IsUIObject(value) then return end
        count = count + 1
        paths[value] = path
        roots[path] = value
    end

    local cursor, scanned = nil, 0
    while count < MAX_GLOBAL_ROOTS and scanned < MAX_GLOBAL_SCAN do
        local succeeded, key, value = pcall(next, _G, cursor)
        if not succeeded or key == nil then break end
        cursor = key
        scanned = scanned + 1
        if type(key) == "string" and IsAddonRootName(key, addonName) then
            Add("_G." .. key, value)
        end
    end

    local variableNames = GetSavedVariableNames(addonName)
    for index = 1, #variableNames do
        local name = variableNames[index]
        Add("SavedVariables." .. name, rawget(_G, name))
    end

    if LibStub then
        local libraryOk, aceAddon = pcall(LibStub, "AceAddon-3.0", true)
        if libraryOk and aceAddon and type(aceAddon.GetAddon) == "function" then
            local addonOk, addon = pcall(aceAddon.GetAddon, aceAddon, addonName, true)
            if addonOk and addon then Add("AceAddon." .. addonName, addon) end
        end
    end
    if extraTarget then Add(extraLabel or "extraObject", extraTarget) end
    return roots, count, variableNames
end

local function SnapshotAddonRuntime(addonName, extraTarget, extraLabel)
    local roots, rootCount, variableNames = BuildAddonRoots(addonName, extraTarget, extraLabel)
    local snapshot = SnapshotTarget(roots, addonName, addonName)
    snapshot.rootCount = rootCount
    snapshot.savedVariableCount = #variableNames
    return snapshot
end

function performance.CollectSavedVariables(addonName)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    if type(addonName) ~= "string" or addonName == "" then return false, nil, ns.L.PERFORMANCE_ADDON_REQUIRED end
    local names = GetSavedVariableNames(addonName)
    local roots, totalBytes, loadedCount = {}, 0, 0
    for index = 1, #names do
        local name = names[index]
        local value = rawget(_G, name)
        local snapshot = SnapshotSavedVariable(value)
        snapshot.name = name
        snapshot.valueType = type(value)
        if value ~= nil then loadedCount = loadedCount + 1 end
        roots[#roots + 1] = snapshot
        totalBytes = totalBytes + snapshot.bytes
    end
    table.sort(roots, function(left, right) return left.bytes > right.bytes end)
    return true, {
        kind = "saved_variables",
        addon = addonName,
        generatedAt = time(),
        roots = roots,
        totalBytes = totalBytes,
        declaredCount = #names,
        loadedCount = loadedCount,
    }
end

local function CopyMetricsDelta(finalMetrics, initialMetrics)
    local delta = {}
    for index = 1, #METRICS do
        local key = METRICS[index].key
        if finalMetrics[key] ~= nil and initialMetrics[key] ~= nil then
            delta[key] = finalMetrics[key] - initialMetrics[key]
        end
    end
    return delta
end

local function BuildStorageDelta(initial, final)
    if not initial or not final then return nil end
    local baseline = {}
    for index = 1, #(initial.roots or {}) do
        baseline[initial.roots[index].name] = initial.roots[index]
    end
    local roots = {}
    for index = 1, #(final.roots or {}) do
        local current = final.roots[index]
        local previous = baseline[current.name] or {}
        roots[#roots + 1] = {
            name = current.name,
            bytes = current.bytes,
            bytesDelta = (current.bytes or 0) - (previous.bytes or 0),
            tables = current.tables,
            tablesDelta = (current.tables or 0) - (previous.tables or 0),
            entries = current.entries,
            entriesDelta = (current.entries or 0) - (previous.entries or 0),
            truncated = current.truncated,
        }
    end
    table.sort(roots, function(left, right)
        return math.abs(left.bytesDelta) > math.abs(right.bytesDelta)
    end)
    return {
        initialBytes = initial.totalBytes,
        finalBytes = final.totalBytes,
        bytesDelta = (final.totalBytes or 0) - (initial.totalBytes or 0),
        initialDeclaredCount = initial.declaredCount or 0,
        finalDeclaredCount = final.declaredCount or 0,
        initialLoadedCount = initial.loadedCount or 0,
        finalLoadedCount = final.loadedCount or 0,
        roots = roots,
    }
end

local function Percentile(values, percentile)
    if #values == 0 then return nil end
    local copy = {}
    for index = 1, #values do copy[index] = values[index] end
    table.sort(copy)
    local position = math.max(1, math.ceil(#copy * percentile))
    return copy[position]
end

local function BuildCaptureSummary(session)
    local lastValues, recentValues, overheadValues, shareValues = {}, {}, {}, {}
    local maximum, total, overheadTotal, activeSamples = 0, 0, 0, 0
    local sampleSpikes = { over1 = 0, over5 = 0, over10 = 0, over50 = 0, over100 = 0 }
    for index = 1, #session.samples do
        local sample = session.samples[index]
        if sample.lastTime then
            lastValues[#lastValues + 1] = sample.lastTime
            maximum = math.max(maximum, sample.lastTime)
            total = total + sample.lastTime
            if sample.lastTime > 0 then activeSamples = activeSamples + 1 end
            if sample.lastTime >= 1 then sampleSpikes.over1 = sampleSpikes.over1 + 1 end
            if sample.lastTime >= 5 then sampleSpikes.over5 = sampleSpikes.over5 + 1 end
            if sample.lastTime >= 10 then sampleSpikes.over10 = sampleSpikes.over10 + 1 end
            if sample.lastTime >= 50 then sampleSpikes.over50 = sampleSpikes.over50 + 1 end
            if sample.lastTime >= 100 then sampleSpikes.over100 = sampleSpikes.over100 + 1 end
        end
        if sample.recentAverage then recentValues[#recentValues + 1] = sample.recentAverage end
        if sample.selfOverhead then
            overheadValues[#overheadValues + 1] = sample.selfOverhead
            overheadTotal = overheadTotal + sample.selfOverhead
        end
        local share = RelativeAddonPercent(sample.lastTime, sample.overallLast, sample.applicationLast)
        if share then shareValues[#shareValues + 1] = share end
    end
    session.summary = {
        sampleCount = #session.samples,
        p50 = Percentile(lastValues, 0.50),
        p95 = Percentile(lastValues, 0.95),
        p99 = Percentile(lastValues, 0.99),
        maximum = maximum,
        recentP95 = Percentile(recentValues, 0.95),
        analyzerOverheadP95 = Percentile(overheadValues, 0.95),
        analyzerOverheadRatio = total + overheadTotal > 0
            and overheadTotal / (total + overheadTotal) * 100 or nil,
        totalSampledTime = total,
        activeSamples = activeSamples,
        relativeShareP50 = Percentile(shareValues, 0.50),
        relativeShareP95 = Percentile(shareValues, 0.95),
        sampleSpikes = sampleSpikes,
        memoryDelta = session.finalMemory and session.initialMemory
            and session.finalMemory - session.initialMemory or nil,
        metricDelta = CopyMetricsDelta(session.finalMetrics or {}, session.initialMetrics or {}),
    }
    session.objectSummary = SerializeObjectSummary(session)
    session.storageSummary = BuildStorageDelta(session.initialStorage, session.finalStorage)
end

local function SampleSession()
    local session = activeSession
    if not session then return end
    if ns.IsCombatBlocked() then
        performance.StopCapture("combat")
        return
    end
    local now = GetTime()
    local metrics = ReadMetricSet(session.addon)
    local overall = ReadOverallMetricSet(C_AddOnProfiler and C_AddOnProfiler.GetOverallMetric)
    local application = ReadOverallMetricSet(C_AddOnProfiler and C_AddOnProfiler.GetApplicationMetric)
    local selfMetrics = ReadMetricSet(ADDON_NAME)
    session.samples[#session.samples + 1] = {
        elapsed = math.max(0, now - session.startedAt),
        lastTime = metrics.lastTime,
        recentAverage = metrics.recentAverage,
        overallLast = overall.lastTime,
        applicationLast = application.lastTime,
        selfOverhead = selfMetrics.lastTime,
    }
    while #session.samples > MAX_SAMPLES do table.remove(session.samples, 1) end

    if now - session.lastMemoryAt >= 1 then
        session.lastMemoryAt = now
        session.finalMemory = ReadMemory(session.addon, true)
    end
    if now - session.lastObjectAt >= OBJECT_SAMPLE_INTERVAL then
        session.lastObjectAt = now
        UpdateObjectState(session, SnapshotAddonRuntime(session.addon,
            session.extraTarget, session.extraTargetLabel), false)
    end
    if session.callback then session.callback("sample", session) end
    if now - session.startedAt >= session.duration then
        performance.StopCapture("complete")
    end
end

function performance.StartCapture(addonName, duration, target, targetLabel, callback)
    if ns.IsCombatBlocked() then return false, ns.L.COMBAT_BLOCKED end
    if activeSession then return false, ns.L.PERFORMANCE_CAPTURE_ALREADY_RUNNING end
    addonName = tostring(addonName or ""):match("^%s*(.-)%s*$")
    if addonName == "" or not IsLoaded(addonName) then return false, ns.L.PERFORMANCE_ADDON_NOT_LOADED end
    if not C_Timer or not C_Timer.NewTicker then return false, ns.L.PERFORMANCE_TIMER_UNAVAILABLE end
    duration = math.max(5, math.min(180, math.floor(tonumber(duration) or 10)))
    local session = {
        kind = "capture",
        addon = addonName,
        startedAt = GetTime(),
        createdAt = time(),
        duration = duration,
        samples = {},
        initialMetrics = ReadMetricSet(addonName),
        initialMemory = ReadMemory(addonName, true),
        extraTarget = (IsUIObject(target) or type(target) == "table") and target or nil,
        extraTargetLabel = targetLabel,
        callback = callback,
        lastMemoryAt = GetTime(),
        lastObjectAt = GetTime(),
    }
    local storageOk, initialStorage = performance.CollectSavedVariables(addonName)
    if storageOk then session.initialStorage = initialStorage end
    session.objectBaseline = SnapshotAddonRuntime(addonName,
        session.extraTarget, session.extraTargetLabel)
    UpdateObjectState(session, session.objectBaseline, true)
    activeSession = session
    SampleSession()
    if activeSession == session then
        session.ticker = C_Timer.NewTicker(SAMPLE_INTERVAL, SampleSession)
    end
    return true, session
end

function performance.StopCapture(reason)
    local session = activeSession
    if not session then return nil end
    activeSession = nil
    if session.ticker and session.ticker.Cancel then session.ticker:Cancel() end
    session.ticker = nil
    session.endedAt = GetTime()
    session.elapsed = math.max(0, session.endedAt - session.startedAt)
    session.stopReason = reason or "manual"
    session.finalMetrics = ReadMetricSet(session.addon)
    session.finalMemory = ReadMemory(session.addon, true)
    local storageOk, finalStorage = performance.CollectSavedVariables(session.addon)
    if storageOk then session.finalStorage = finalStorage end
    UpdateObjectState(session, SnapshotAddonRuntime(session.addon,
        session.extraTarget, session.extraTargetLabel), false)
    BuildCaptureSummary(session)
    session.extraTarget = nil
    session.extraTargetLabel = nil
    session.objectState = nil
    session.objectBaseline = nil
    session.objectLastSnapshot = nil
    local callback = session.callback
    session.callback = nil
    table.insert(sessions, 1, session)
    while #sessions > MAX_SESSIONS do sessions[#sessions] = nil end
    if callback then callback("finished", session) end
    return session
end

function performance.RunBenchmark(path, iterations, asMethod)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    if not C_AddOnProfiler or type(C_AddOnProfiler.MeasureCall) ~= "function" then
        return false, nil, ns.L.PERFORMANCE_MEASURE_UNAVAILABLE
    end
    local succeeded, owner, key, errorMessage = ns.ObjectInspector.ResolveFunctionTarget(path)
    if not succeeded then return false, nil, errorMessage end
    local readSucceeded, target = pcall(function() return owner[key] end)
    if not readSucceeded or type(target) ~= "function" or IsSecret(target) then
        return false, nil, ns.L.FUNCTION_NOT_FOUND
    end
    iterations = math.max(1, math.min(200, math.floor(tonumber(iterations) or 20)))
    local warmups = math.min(3, iterations)
    for _ = 1, warmups do
        local warmed = asMethod and pcall(C_AddOnProfiler.MeasureCall, target, owner)
            or pcall(C_AddOnProfiler.MeasureCall, target)
        if not warmed then break end
    end

    local samples, elapsedValues, allocatedValues, netValues = {}, {}, {}, {}
    for index = 1, iterations do
        local called, result
        if asMethod then
            called, result = pcall(C_AddOnProfiler.MeasureCall, target, owner)
        else
            called, result = pcall(C_AddOnProfiler.MeasureCall, target)
        end
        if not called or type(result) ~= "table" then
            return false, nil, tostring(result or ns.L.PERFORMANCE_BENCHMARK_FAILED)
        end
        local elapsed = SafeNumber(result.elapsedMilliseconds) or 0
        local allocated = SafeNumber(result.allocatedBytes) or 0
        local deallocated = SafeNumber(result.deallocatedBytes) or 0
        local net = allocated - deallocated
        samples[#samples + 1] = {
            index = index,
            elapsed = elapsed,
            ticks = SafeNumber(result.elapsedTicks),
            allocated = allocated,
            deallocated = deallocated,
            net = net,
            events = type(result.events) == "table" and result.events or nil,
        }
        elapsedValues[#elapsedValues + 1] = elapsed
        allocatedValues[#allocatedValues + 1] = allocated
        netValues[#netValues + 1] = net
    end
    local totalElapsed, totalAllocated, totalNet = 0, 0, 0
    for index = 1, #samples do
        totalElapsed = totalElapsed + samples[index].elapsed
        totalAllocated = totalAllocated + samples[index].allocated
        totalNet = totalNet + samples[index].net
    end
    lastBenchmark = {
        kind = "function_benchmark",
        createdAt = time(),
        path = path,
        iterations = iterations,
        asMethod = asMethod and true or false,
        samples = samples,
        summary = {
            mean = totalElapsed / iterations,
            p50 = Percentile(elapsedValues, 0.50),
            p95 = Percentile(elapsedValues, 0.95),
            p99 = Percentile(elapsedValues, 0.99),
            maximum = Percentile(elapsedValues, 1),
            allocatedPerCall = totalAllocated / iterations,
            allocatedP95 = Percentile(allocatedValues, 0.95),
            netPerCall = totalNet / iterations,
            netP95 = Percentile(netValues, 0.95),
            callsPerSecond = totalElapsed > 0 and iterations * 1000 / totalElapsed or nil,
        },
    }
    return true, lastBenchmark
end

function performance.GetSessions() return sessions end
function performance.GetActiveSession() return activeSession end
function performance.GetLastBenchmark() return lastBenchmark end
function performance.IsCapturing() return activeSession ~= nil end
function performance.GetMetricDefinitions() return METRICS end

ns.RegisterCombatShutdown(function() performance.StopCapture("combat") end)
ns.Performance = performance
