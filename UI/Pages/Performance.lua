local _, ns = ...
local L = ns.L

local HEALTH_ROW_HEIGHT = 48
local SESSION_ROW_HEIGHT = 46
local HEALTH_LIST_WIDTH = 410
local SESSION_LIST_WIDTH = 300
local VISIBLE_ROWS = 12

local FINDING_TEXT = {
    freeze_1000 = L.PERFORMANCE_FINDING_FREEZE_1000,
    freeze_500 = L.PERFORMANCE_FINDING_FREEZE_500,
    spike_100 = L.PERFORMANCE_FINDING_SPIKE_100,
    spike_50 = L.PERFORMANCE_FINDING_SPIKE_50,
    sustained_high = L.PERFORMANCE_FINDING_SUSTAINED_HIGH,
    sustained_medium = L.PERFORMANCE_FINDING_SUSTAINED_MEDIUM,
    encounter_hot = L.PERFORMANCE_FINDING_ENCOUNTER,
    bursty = L.PERFORMANCE_FINDING_BURSTY,
}

local function FormatMilliseconds(value)
    if value == nil then return "--" end
    if value >= 0 and value < 0.001 then return L.PERFORMANCE_VALUE_BELOW_RESOLUTION end
    if value < 0.1 then return string.format("%.3fms", value) end
    if value < 10 then return string.format("%.2fms", value) end
    return string.format("%.1fms", value)
end

local function FormatPercent(value)
    if value == nil then return "--" end
    if value > 0 and value < 0.01 then return "<0.01%" end
    if value < 0.1 then return string.format("%.2f%%", value) end
    if value < 10 then return string.format("%.1f%%", value) end
    return string.format("%.0f%%", value)
end

local function FormatBytes(value)
    if value == nil then return "--" end
    local absolute = math.abs(value)
    local sign = value > 0 and "+" or value < 0 and "-" or ""
    if absolute >= 1024 * 1024 then
        return string.format("%s%.2f MB", sign, absolute / 1024 / 1024)
    elseif absolute >= 1024 then
        return string.format("%s%.1f KB", sign, absolute / 1024)
    end
    return string.format("%s%d B", sign, math.floor(absolute + 0.5))
end

local function FormatByteValue(value)
    if value == nil then return "--" end
    local absolute = math.abs(value)
    if absolute >= 1024 * 1024 then return string.format("%.2f MB", absolute / 1024 / 1024) end
    if absolute >= 1024 then return string.format("%.1f KB", absolute / 1024) end
    return string.format("%d B", math.floor(absolute + 0.5))
end

local function FormatMemoryKB(value)
    if value == nil then return "--" end
    local absolute = math.abs(value)
    local sign = value > 0 and "+" or value < 0 and "-" or ""
    return absolute >= 1024 and string.format("%s%.1f MB", sign, absolute / 1024)
        or string.format("%s%.0f KB", sign, absolute)
end

local function FormatMemoryValue(value)
    if value == nil then return "--" end
    return value >= 1024 and string.format("%.1f MB", value / 1024)
        or string.format("%.0f KB", value)
end

local function RiskText(severity)
    if severity >= 4 then return L.PERFORMANCE_RISK_CRITICAL end
    if severity >= 3 then return L.PERFORMANCE_RISK_HIGH end
    if severity >= 2 then return L.PERFORMANCE_RISK_WATCH end
    if severity >= 1 then return L.PERFORMANCE_RISK_HISTORY end
    return L.PERFORMANCE_RISK_NONE
end

local function RiskColor(severity)
    if severity >= 4 then return 1, 0.24, 0.30 end
    if severity >= 3 then return 1, 0.48, 0.24 end
    if severity >= 2 then return 0.95, 0.72, 0.24 end
    if severity >= 1 then return 0.68, 0.72, 0.76 end
    return 0.42, 0.76, 0.43
end

local function CreateLineInput(parent, ui, text)
    local panel = ui.CreatePanel(parent, ui.editorR, ui.editorG, ui.editorB, 1)
    local editBox = CreateFrame("EditBox", nil, panel)
    editBox:SetAutoFocus(false)
    editBox:SetFontObject(ChatFontNormal)
    editBox:SetTextColor(0.94, 0.95, 0.96)
    editBox:SetTextInsets(9, 9, 0, 0)
    editBox:SetPoint("TOPLEFT", 1, -1)
    editBox:SetPoint("BOTTOMRIGHT", -1, 1)
    editBox:SetText(text or "")
    editBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    editBox:SetScript("OnEditFocusGained", function() ui.SetBorderColor(panel, true, 0.75) end)
    editBox:SetScript("OnEditFocusLost", function() ui.SetBorderColor(panel, false) end)
    panel.editBox = editBox
    return panel
end

local function AddSection(lines, title)
    if #lines > 0 then lines[#lines + 1] = "" end
    lines[#lines + 1] = title
end

local function FormatFinding(finding)
    local formatText = FINDING_TEXT[finding.code]
    return formatText and string.format(formatText, finding.value) or finding.code
end

local function FormatHealthReport(snapshot, entry)
    if not entry then return L.PERFORMANCE_SELECT_ADDON end
    local metrics = entry.metrics or {}
    local lines = {
        L.PERFORMANCE_BASELINE,
        entry.title .. "  (" .. entry.name .. ")",
    }
    AddSection(lines, L.PERFORMANCE_CONCLUSION)
    lines[#lines + 1] = L.PERFORMANCE_RISK .. "：" .. RiskText(entry.severity or 0)
    if #(entry.findings or {}) == 0 then
        lines[#lines + 1] = L.PERFORMANCE_NO_FINDINGS
    else
        for index = 1, #entry.findings do
            lines[#lines + 1] = "- " .. FormatFinding(entry.findings[index])
        end
    end
    AddSection(lines, L.PERFORMANCE_CURRENT_LOAD)
    lines[#lines + 1] = L.PERFORMANCE_RECENT .. "：" .. FormatMilliseconds(metrics.recentAverage)
    lines[#lines + 1] = L.PERFORMANCE_SHARE .. "：" .. FormatPercent(entry.addonShare)
    lines[#lines + 1] = string.format(L.PERFORMANCE_ALL_ADDONS_SHARE,
        FormatPercent(entry.overallShare))
    lines[#lines + 1] = L.MEMORY_COLUMN .. "：" .. FormatMemoryValue(entry.memory)
    AddSection(lines, L.PERFORMANCE_LONG_TERM)
    lines[#lines + 1] = L.PERFORMANCE_SESSION_AVERAGE .. "：" .. FormatMilliseconds(metrics.sessionAverage)
    lines[#lines + 1] = L.PERFORMANCE_ENCOUNTER .. "：" .. FormatMilliseconds(metrics.encounterAverage)
    lines[#lines + 1] = L.PERFORMANCE_PEAK .. "：" .. FormatMilliseconds(metrics.peakTime)
    lines[#lines + 1] = string.format("1/5/10ms：%d / %d / %d",
        metrics.over1 or 0, metrics.over5 or 0, metrics.over10 or 0)
    lines[#lines + 1] = string.format("50/100/500/1000ms：%d / %d / %d / %d",
        metrics.over50 or 0, metrics.over100 or 0, metrics.over500 or 0, metrics.over1000 or 0)
    AddSection(lines, L.PERFORMANCE_CAPABILITIES)
    lines[#lines + 1] = (snapshot.profilerEnabled and "- " .. L.PERFORMANCE_NATIVE_READY
        or "- " .. L.PERFORMANCE_NATIVE_UNAVAILABLE)
    lines[#lines + 1] = (snapshot.scriptProfile and "- " .. L.PERFORMANCE_DEEP_READY
        or "- " .. L.PERFORMANCE_DEEP_UNAVAILABLE)
    if snapshot.cpuBound ~= nil then
        lines[#lines + 1] = "- " .. L.PERFORMANCE_CPU_BOUND .. "："
            .. (snapshot.cpuBound and L.PERFORMANCE_CPU_BOUND_YES or L.PERFORMANCE_CPU_BOUND_NO)
    end
    AddSection(lines, L.PERFORMANCE_NEXT_ACTION)
    lines[#lines + 1] = L.PERFORMANCE_START_GUIDE
    return table.concat(lines, "\n")
end

local function AddHotFunctions(lines, summary)
    AddSection(lines, L.PERFORMANCE_FUNCTION_HOTSPOTS)
    local hotFunctions = summary and summary.hotFunctions or {}
    if #hotFunctions == 0 then
        lines[#lines + 1] = summary and summary.functionsDiscovered > 0
            and L.PERFORMANCE_NO_ACTIONABLE_FINDINGS or L.PERFORMANCE_NAMESPACE_MISSING
        return
    end
    for index = 1, math.min(30, #hotFunctions) do
        local item = hotFunctions[index]
        lines[#lines + 1] = string.format(L.PERFORMANCE_FUNCTION_ROW,
            index, FormatMilliseconds(item.elapsed), FormatMilliseconds(item.inclusive),
            item.path, item.calls or 0,
            FormatMilliseconds(item.average))
    end
end

local function AddHotFrames(lines, summary)
    AddSection(lines, L.PERFORMANCE_FRAME_HOTSPOTS)
    local hotFrames = summary and summary.hotFrames or {}
    if #hotFrames == 0 then
        lines[#lines + 1] = L.PERFORMANCE_NO_ACTIONABLE_FINDINGS
        return
    end
    for index = 1, math.min(30, #hotFrames) do
        local item = hotFrames[index]
        lines[#lines + 1] = string.format(L.PERFORMANCE_FRAME_ROW,
            index, FormatMilliseconds(item.elapsed), item.name or item.path,
            item.calls or 0, item.onUpdate and "  OnUpdate" or "")
        if item.sourceFile then lines[#lines + 1] = "    " .. item.sourceFile end
    end
end

local function FormatCPUReport(session)
    if not session or not session.summary then return L.PERFORMANCE_NO_CAPTURE end
    local summary = session.summary
    local delta = summary.metricDelta or {}
    local sampleSpikes = summary.sampleSpikes or {}
    local lines = {
        L.PERFORMANCE_RESULT_CPU,
        session.addon,
        "",
        L.PERFORMANCE_P50 .. "：" .. FormatMilliseconds(summary.p50),
        L.PERFORMANCE_P95 .. "：" .. FormatMilliseconds(summary.p95),
        L.PERFORMANCE_P99 .. "：" .. FormatMilliseconds(summary.p99),
        L.PERFORMANCE_MAXIMUM .. "：" .. FormatMilliseconds(summary.maximum),
        L.PERFORMANCE_RECENT_P95 .. "：" .. FormatMilliseconds(summary.recentP95),
        L.PERFORMANCE_ANALYZER_OVERHEAD .. "：" .. FormatMilliseconds(summary.analyzerOverheadP95),
        string.format(L.PERFORMANCE_SAMPLE_ACTIVITY, summary.activeSamples or 0,
            summary.sampleCount or 0, FormatMilliseconds(summary.totalSampledTime)),
        string.format(L.PERFORMANCE_RELATIVE_LOAD,
            FormatPercent(summary.relativeShareP50), FormatPercent(summary.relativeShareP95)),
        string.format(L.PERFORMANCE_SAMPLE_SPIKES, sampleSpikes.over1 or 0,
            sampleSpikes.over5 or 0, sampleSpikes.over10 or 0,
            sampleSpikes.over50 or 0, sampleSpikes.over100 or 0),
        string.format(L.PERFORMANCE_COUNTER_SPIKES, delta.over10 or 0,
            delta.over50 or 0, delta.over100 or 0, delta.over500 or 0, delta.over1000 or 0),
    }
    if summary.analyzerOverheadRatio and summary.analyzerOverheadRatio >= 10 then
        lines[#lines + 1] = string.format(L.PERFORMANCE_OVERHEAD_WARNING,
            FormatPercent(summary.analyzerOverheadRatio))
    else
        lines[#lines + 1] = string.format(L.PERFORMANCE_OVERHEAD_OK,
            FormatPercent(summary.analyzerOverheadRatio))
    end
    AddHotFunctions(lines, session.objectSummary)
    AddHotFrames(lines, session.objectSummary)
    return table.concat(lines, "\n")
end

local function FormatObjectSummary(summary)
    if not summary then return L.PERFORMANCE_NO_CAPTURE end
    local lines = {
        L.PERFORMANCE_RESULT_OBJECTS,
        string.format(L.PERFORMANCE_NAMESPACE_COVERAGE,
            summary.rootsDiscovered or 0, summary.functionReferences or 0,
            summary.attributedFrames or 0),
        "",
        string.format(L.PERFORMANCE_OBJECT_COUNTS, summary.initialObjects or 0,
            summary.finalObjects or 0, summary.peakObjects or 0, summary.newlyObserved or 0),
        string.format(L.PERFORMANCE_OBJECT_TRANSITIONS, summary.hiddenTransitions or 0,
            summary.activations or 0, FormatPercent(summary.reuseRate)),
        L.PERFORMANCE_ONUPDATE .. "：" .. (summary.onUpdate or 0),
        string.format(L.PERFORMANCE_OBJECT_VISIBLE, summary.visible or 0),
        string.format(L.PERFORMANCE_GRAPH_COVERAGE,
            summary.graph and summary.graph.tables or 0,
            summary.graph and summary.graph.entries or 0,
            summary.graph and summary.graph.functions or 0),
        string.format(L.PERFORMANCE_FRAME_COVERAGE,
            summary.framesScanned or 0, summary.attributedFrames or 0),
    }
    if summary.truncated then lines[#lines + 1] = L.PERFORMANCE_OBJECT_TRUNCATED end

    local function AddCounts(title, counts)
        local rows = {}
        for key, value in pairs(counts or {}) do
            rows[#rows + 1] = { key = key, value = value }
        end
        table.sort(rows, function(left, right)
            if left.value ~= right.value then return left.value > right.value end
            return left.key < right.key
        end)
        if #rows > 0 then
            AddSection(lines, title)
            for index = 1, #rows do
                lines[#lines + 1] = rows[index].key .. "：" .. rows[index].value
            end
        end
    end
    AddCounts(L.PERFORMANCE_OBJECT_TYPES, summary.byType)
    AddCounts(L.PERFORMANCE_OBJECT_SOURCES, summary.bySource)

    AddSection(lines, L.PERFORMANCE_CLOSURE_ANALYSIS)
    lines[#lines + 1] = string.format(L.PERFORMANCE_FUNCTION_COUNTS,
        summary.functionsDiscovered or 0, summary.functionReferences or 0,
        summary.newFunctionInstances or 0, summary.functionReplacements or 0)
    if #(summary.functionChurn or {}) == 0 then
        lines[#lines + 1] = L.PERFORMANCE_NO_FUNCTION_CHURN
    else
        for index = 1, math.min(20, #summary.functionChurn) do
            local item = summary.functionChurn[index]
            lines[#lines + 1] = string.format(L.PERFORMANCE_FUNCTION_CHURN,
                item.path, item.replacements)
        end
    end

    AddSection(lines, L.PERFORMANCE_POOLS)
    if #(summary.pools or {}) > 0 then
        for index = 1, math.min(12, #summary.pools) do
            local pool = summary.pools[index]
            lines[#lines + 1] = pool.path
            lines[#lines + 1] = string.format(L.PERFORMANCE_POOL_ACTIVE,
                pool.initialActive or 0, pool.finalActive or 0, pool.peakActive or 0)
            lines[#lines + 1] = string.format(L.PERFORMANCE_POOL_REUSE,
                pool.acquisitions or 0, pool.releases or 0, pool.reuseRate or 0)
        end
    else
        lines[#lines + 1] = L.PERFORMANCE_NO_POOLS
    end

    AddSection(lines, L.PERFORMANCE_EVIDENCE_LIMITS)
    if (summary.rootsDiscovered or 0) == 0 then
        lines[#lines + 1] = L.PERFORMANCE_NAMESPACE_MISSING
    end
    if not summary.frameScanAvailable then
        lines[#lines + 1] = L.PERFORMANCE_FRAME_SCAN_UNAVAILABLE
    end
    lines[#lines + 1] = L.PERFORMANCE_TRANSIENT_OBJECT_LIMIT
    lines[#lines + 1] = L.PERFORMANCE_POOL_HEURISTIC
    return table.concat(lines, "\n")
end

local function FormatCaptureReport(session)
    if not session or not session.summary then return L.PERFORMANCE_NO_CAPTURE end
    local summary = session.summary
    local objectSummary = session.objectSummary or {}
    local sampleSpikes = summary.sampleSpikes or {}
    local lines = {
        L.PERFORMANCE_CONCLUSION,
        session.addon,
        string.format(L.PERFORMANCE_CAPTURE_META, session.elapsed or 0,
            summary.sampleCount or 0, session.stopReason or "complete"),
    }
    AddSection(lines, L.PERFORMANCE_CONCLUSION)
    if (sampleSpikes.over100 or 0) > 0 then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_FINDING_SPIKE_100,
            sampleSpikes.over100)
    elseif (sampleSpikes.over50 or 0) > 0 then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_FINDING_SPIKE_50,
            sampleSpikes.over50)
    elseif (sampleSpikes.over10 or 0) == 0 then
        lines[#lines + 1] = "- " .. L.PERFORMANCE_CAPTURE_NO_SPIKE
    end
    lines[#lines + 1] = "- " .. L.PERFORMANCE_P95 .. "：" .. FormatMilliseconds(summary.p95)
    lines[#lines + 1] = "- " .. L.PERFORMANCE_MAXIMUM .. "：" .. FormatMilliseconds(summary.maximum)
    lines[#lines + 1] = "- " .. L.PERFORMANCE_MEMORY_DELTA .. "：" .. FormatMemoryKB(summary.memoryDelta)

    AddSection(lines, L.PERFORMANCE_NEXT_ACTION)
    local hotFunction = objectSummary.hotFunctions and objectSummary.hotFunctions[1]
    local hotFrame = objectSummary.hotFrames and objectSummary.hotFrames[1]
    local hasAction
    if hotFunction then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_ACTION_HOT_FUNCTION,
            hotFunction.path, FormatMilliseconds(hotFunction.elapsed))
        hasAction = true
    end
    if hotFrame then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_ACTION_HOT_FRAME,
            hotFrame.name or hotFrame.path, FormatMilliseconds(hotFrame.elapsed),
            hotFrame.calls or 0)
        hasAction = true
    end
    if (objectSummary.functionReplacements or 0) > 0 then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_ACTION_CLOSURE,
            objectSummary.functionReplacements)
        hasAction = true
    end
    if session.storageSummary and (session.storageSummary.bytesDelta or 0) > 0 then
        lines[#lines + 1] = "- " .. string.format(L.PERFORMANCE_ACTION_STORAGE,
            FormatBytes(session.storageSummary.bytesDelta))
        hasAction = true
    end
    if not hasAction then lines[#lines + 1] = "- " .. L.PERFORMANCE_NO_ACTIONABLE_FINDINGS end

    AddSection(lines, L.PERFORMANCE_RAW_EVIDENCE)
    lines[#lines + 1] = L.PERFORMANCE_P50 .. "：" .. FormatMilliseconds(summary.p50)
    lines[#lines + 1] = L.PERFORMANCE_P95 .. "：" .. FormatMilliseconds(summary.p95)
    lines[#lines + 1] = L.PERFORMANCE_P99 .. "：" .. FormatMilliseconds(summary.p99)
    lines[#lines + 1] = L.PERFORMANCE_MAXIMUM .. "：" .. FormatMilliseconds(summary.maximum)
    lines[#lines + 1] = string.format(L.PERFORMANCE_RELATIVE_LOAD,
        FormatPercent(summary.relativeShareP50), FormatPercent(summary.relativeShareP95))
    lines[#lines + 1] = string.format(L.PERFORMANCE_NAMESPACE_COVERAGE,
        objectSummary.rootsDiscovered or 0, objectSummary.functionReferences or 0,
        objectSummary.attributedFrames or 0)
    return table.concat(lines, "\n")
end

local function FormatStorageDeltaReport(session)
    if not session or not session.storageSummary then return L.PERFORMANCE_NO_CAPTURE end
    local summary = session.storageSummary
    local lines = {
        L.PERFORMANCE_STORAGE_DELTA,
        session.addon,
        "",
        L.PERFORMANCE_STORAGE_ESTIMATE_NOTE,
    }
    if (summary.finalDeclaredCount or 0) == 0 then
        lines[#lines + 1] = ""
        lines[#lines + 1] = L.PERFORMANCE_STORAGE_NOT_DECLARED
        return table.concat(lines, "\n")
    end
    if (summary.finalLoadedCount or 0) == 0 then
        lines[#lines + 1] = ""
        lines[#lines + 1] = string.format(L.PERFORMANCE_STORAGE_NOT_CREATED,
            summary.finalDeclaredCount or 0)
        return table.concat(lines, "\n")
    end
    lines[#lines + 1] = ""
    lines[#lines + 1] = string.format(L.PERFORMANCE_STORAGE_DELTA_TOTAL,
        FormatByteValue(summary.initialBytes or 0), FormatByteValue(summary.finalBytes or 0),
        FormatBytes(summary.bytesDelta or 0))
    if (summary.bytesDelta or 0) == 0 then
        lines[#lines + 1] = L.PERFORMANCE_STORAGE_NO_CHANGE
    end
    for index = 1, #(summary.roots or {}) do
        local root = summary.roots[index]
        lines[#lines + 1] = string.format(L.PERFORMANCE_STORAGE_DELTA_ROOT,
            root.name, FormatBytes(root.bytesDelta), root.tablesDelta or 0,
            root.entriesDelta or 0)
        if root.truncated then lines[#lines + 1] = "    " .. L.PERFORMANCE_OBJECT_TRUNCATED end
    end
    return table.concat(lines, "\n")
end

local function FormatFullCaptureReport(session)
    if not session then return L.PERFORMANCE_NO_CAPTURE end
    local sections = {
        FormatCaptureReport(session),
        FormatCPUReport(session),
        FormatObjectSummary(session.objectSummary),
        FormatStorageDeltaReport(session),
    }
    local samples = { L.PERFORMANCE_RAW_TIMELINE }
    for index = 1, #(session.samples or {}) do
        local sample = session.samples[index]
        samples[#samples + 1] = string.format(L.PERFORMANCE_RAW_SAMPLE,
            sample.elapsed or 0, FormatMilliseconds(sample.lastTime),
            FormatMilliseconds(sample.recentAverage), FormatMilliseconds(sample.overallLast),
            FormatMilliseconds(sample.applicationLast), FormatMilliseconds(sample.selfOverhead))
    end
    sections[#sections + 1] = table.concat(samples, "\n")
    return table.concat(sections, "\n\n")
end

local function FormatBenchmarkReport(result)
    if not result then return L.PERFORMANCE_BENCHMARK_READY end
    local summary = result.summary
    local lines = {
        L.PERFORMANCE_REPORT,
        "",
        result.addon or L.UNKNOWN,
        result.path,
        string.format(L.PERFORMANCE_BENCHMARK_META, result.iterations,
            result.asMethod and L.PERFORMANCE_AS_METHOD or L.PERFORMANCE_NO_ARGUMENTS),
        "",
        L.PERFORMANCE_EVIDENCE_EXACT,
        L.PERFORMANCE_MEAN .. "：" .. FormatMilliseconds(summary.mean),
        L.PERFORMANCE_P50 .. "：" .. FormatMilliseconds(summary.p50),
        L.PERFORMANCE_P95 .. "：" .. FormatMilliseconds(summary.p95),
        L.PERFORMANCE_P99 .. "：" .. FormatMilliseconds(summary.p99),
        L.PERFORMANCE_MAXIMUM .. "：" .. FormatMilliseconds(summary.maximum),
        L.PERFORMANCE_ALLOCATED .. "：" .. FormatBytes(summary.allocatedPerCall),
        L.PERFORMANCE_NET_ALLOCATION .. "：" .. FormatBytes(summary.netPerCall),
        L.PERFORMANCE_THROUGHPUT .. "："
            .. (summary.callsPerSecond and string.format("%.1f", summary.callsPerSecond) or "--"),
    }
    return table.concat(lines, "\n")
end

function ns.CreatePerformancePage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local modes = {
        { key = "health", label = L.PERFORMANCE_HEALTH, width = 88 },
        { key = "capture", label = L.PERFORMANCE_CAPTURE, width = 88 },
        { key = "functionLab", label = L.PERFORMANCE_FUNCTION, width = 88 },
    }
    local modeTabs, views = {}, {}
    local mode = "health"
    local previousTab
    for index = 1, #modes do
        local definition = modes[index]
        local tab = ui.CreateViewTab(page, definition.label)
        ui.FitViewTab(tab, definition.width, 30)
        if previousTab then tab:SetPoint("LEFT", previousTab, "RIGHT", 8, 0)
        else tab:SetPoint("TOPLEFT", 14, -84) end
        modeTabs[definition.key] = tab
        previousTab = tab
    end

    local selectedAddon
    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetJustifyH("RIGHT")
    status:SetTextColor(0.55, 0.60, 0.65, 0.92)
    local function SetStatus(text, errorState)
        status:SetText(text or "")
        status:SetTextColor(errorState and ui.accentR or 0.55,
            errorState and ui.accentG or 0.60,
            errorState and ui.accentB or 0.65, 0.92)
        status:SetShown(errorState and true or false)
    end

    local selectedHealthEntry
    local healthSnapshot
    local selectedSession
    local benchmarkResult
    local SetMode

    local changeAddon = ui.CreateButton(page, 88, L.PERFORMANCE_CHANGE_ADDON, "ghost")
    changeAddon:SetPoint("TOPRIGHT", -14, -84)
    status:SetPoint("LEFT", previousTab, "RIGHT", 24, 0)
    status:SetPoint("RIGHT", changeAddon, "LEFT", -12, 0)
    status:Hide()
    changeAddon:Hide()

    local healthView = CreateFrame("Frame", nil, page)
    healthView:SetPoint("TOPLEFT", 0, -126)
    healthView:SetPoint("BOTTOMRIGHT")
    views.health = healthView
    local healthSearch = CreateLineInput(healthView, ui, "")
    healthSearch:SetPoint("TOPLEFT", 14, 0)
    healthSearch:SetSize(300, 30)
    local searchHint = healthSearch:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    searchHint:SetPoint("LEFT", 10, 0)
    searchHint:SetText(L.PERFORMANCE_SEARCH_ADDON)
    searchHint:SetTextColor(1, 1, 1, 0.30)
    local refreshHealth = ui.CreateButton(healthView, 108, L.PERFORMANCE_REFRESH_HEALTH, "ghost")
    refreshHealth:SetPoint("TOPRIGHT", -14, 0)
    local exportHealth = ui.CreateButton(healthView, 92, L.PERFORMANCE_EXPORT, "ghost")
    exportHealth:SetPoint("RIGHT", refreshHealth, "LEFT", -8, 0)
    local healthList = ui.CreatePanel(healthView, ui.editorR, ui.editorG, ui.editorB, 0.78)
    healthList:SetPoint("TOPLEFT", 14, -44)
    healthList:SetPoint("BOTTOMLEFT", 14, 14)
    healthList:SetWidth(HEALTH_LIST_WIDTH)
    local healthScroll = ui.CreateScrollArea(healthList, 8, 8, 7, 8)
    local healthContent = CreateFrame("Frame", nil, healthScroll)
    healthContent:SetSize(HEALTH_LIST_WIDTH - 26, 1)
    healthScroll:SetScrollChild(healthContent)
    local healthEmpty = healthList:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    healthEmpty:SetPoint("TOP", 0, -24)
    healthEmpty:SetText(L.PERFORMANCE_NO_ADDONS)
    healthEmpty:SetTextColor(1, 1, 1, 0.34)
    local healthReport = ui.CreateTextArea(healthView, true)
    healthReport:SetPoint("TOPLEFT", healthList, "TOPRIGHT", 12, 0)
    healthReport:SetPoint("BOTTOMRIGHT", -14, 14)
    healthReport.editBox:SetWidth(ui.windowWidth - HEALTH_LIST_WIDTH - 66)
    ui.SetReadOnlyText(healthReport, L.PERFORMANCE_SELECT_ADDON)
    local healthRows = {}
    local filteredAddons = {}

    local function ApplyHealthRow(row)
        ui.SetListRowState(row, row.entry == selectedHealthEntry, row.isHovered)
    end

    local function CreateHealthRow(index)
        local row = ui.CreateListRow(healthContent, HEALTH_ROW_HEIGHT)
        row:SetWidth(HEALTH_LIST_WIDTH - 26)
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 10, -7)
        name:SetPoint("TOPRIGHT", -78, -7)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name
        local metrics = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        metrics:SetPoint("BOTTOMLEFT", 10, 6)
        metrics:SetPoint("BOTTOMRIGHT", -10, 6)
        metrics:SetJustifyH("LEFT")
        metrics:SetWordWrap(false)
        metrics:SetTextColor(1, 1, 1, 0.40)
        row.metrics = metrics
        local risk = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        risk:SetPoint("TOPRIGHT", -10, -7)
        risk:SetWidth(64)
        risk:SetJustifyH("RIGHT")
        row.risk = risk
        row:SetScript("OnEnter", function(self) self.isHovered = true ApplyHealthRow(self) end)
        row:SetScript("OnLeave", function(self) self.isHovered = nil ApplyHealthRow(self) end)
        row:SetScript("OnClick", function(self)
            local changed = selectedAddon ~= self.entry.name
            selectedHealthEntry = self.entry
            selectedAddon = self.entry.name
            if changed then
                selectedSession = nil
                benchmarkResult = nil
            end
            ui.SetReadOnlyText(healthReport, FormatHealthReport(healthSnapshot, self.entry))
            for rowIndex = 1, #healthRows do ApplyHealthRow(healthRows[rowIndex]) end
            for key, tab in pairs(modeTabs) do
                if key ~= "health" then ui.SetButtonEnabled(tab, true) end
            end
            if SetMode then SetMode("capture") end
        end)
        healthRows[index] = row
        return row
    end

    local function RefreshHealthRows()
        for index = 1, #healthRows do healthRows[index]:Hide() end
        local addons = filteredAddons
        local offset = healthScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then offset = 0 end
        local first = math.floor((offset or 0) / HEALTH_ROW_HEIGHT) + 1
        local last = math.min(#addons, first + VISIBLE_ROWS - 1)
        local pool = 0
        for addonIndex = first, last do
            pool = pool + 1
            local entry = addons[addonIndex]
            local row = healthRows[pool] or CreateHealthRow(pool)
            row.entry = entry
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((addonIndex - 1) * HEALTH_ROW_HEIGHT))
            row.name:SetText(entry.title)
            row.metrics:SetText(string.format(L.PERFORMANCE_HEALTH_ROW,
                FormatMilliseconds(entry.metrics.recentAverage),
                FormatMilliseconds(entry.metrics.encounterAverage),
                FormatMilliseconds(entry.metrics.peakTime)))
            row.risk:SetText(RiskText(entry.severity or 0))
            row.risk:SetTextColor(RiskColor(entry.severity or 0))
            ApplyHealthRow(row)
            row:Show()
        end
        healthContent:SetHeight(math.max(1, #addons * HEALTH_ROW_HEIGHT))
        healthScroll:UpdateScrollChildRect()
        healthEmpty:SetShown(#addons == 0)
        ui.SetButtonEnabled(exportHealth, selectedHealthEntry ~= nil)
    end
    healthScroll.onVerticalScrollChanged = RefreshHealthRows

    local function RebuildFilteredAddons()
        local query = healthSearch.editBox:GetText():lower():match("^%s*(.-)%s*$")
        filteredAddons = {}
        local addons = healthSnapshot and healthSnapshot.addons or {}
        for index = 1, #addons do
            local entry = addons[index]
            if query == "" or entry.name:lower():find(query, 1, true)
                or entry.title:lower():find(query, 1, true) then
                filteredAddons[#filteredAddons + 1] = entry
            end
        end
        healthScroll:SetVerticalScroll(0)
        searchHint:SetShown(query == "")
        RefreshHealthRows()
    end
    healthSearch.editBox:SetScript("OnTextChanged", RebuildFilteredAddons)
    healthSearch.editBox:SetScript("OnEditFocusGained", function()
        searchHint:Hide()
        ui.SetBorderColor(healthSearch, true, 0.75)
    end)
    healthSearch.editBox:SetScript("OnEditFocusLost", function(self)
        searchHint:SetShown(self:GetText() == "")
        ui.SetBorderColor(healthSearch, false)
    end)

    local function RefreshHealth()
        local succeeded, snapshot, errorMessage = ns.Performance.CollectHealth()
        if not succeeded then SetStatus(errorMessage, true) return end
        healthSnapshot = snapshot
        if selectedAddon then
            selectedHealthEntry = nil
            for index = 1, #snapshot.addons do
                if snapshot.addons[index].name == selectedAddon then
                    selectedHealthEntry = snapshot.addons[index]
                    break
                end
            end
        end
        ui.SetReadOnlyText(healthReport, FormatHealthReport(snapshot, selectedHealthEntry))
        RebuildFilteredAddons()
        SetStatus(L.PERFORMANCE_UPDATED, false)
    end
    refreshHealth:SetScript("OnClick", RefreshHealth)
    exportHealth:SetScript("OnClick", function()
        if healthSnapshot and selectedHealthEntry then
            local report = {
                addon = selectedHealthEntry,
                cpuBound = healthSnapshot.cpuBound,
                capturedAt = healthSnapshot.capturedAt,
                overall = healthSnapshot.overall,
                application = healthSnapshot.application,
            }
            ui.ExportText("performance_health", selectedHealthEntry.title,
                function() return ns.SerializeForExport(report) end,
                { addon = selectedAddon, evidence = "exact+derived" })
        end
    end)

    local captureView = CreateFrame("Frame", nil, page)
    captureView:SetPoint("TOPLEFT", 0, -126)
    captureView:SetPoint("BOTTOMRIGHT")
    views.capture = captureView
    local durationLabel = ui.CreateSectionLabel(captureView, L.PERFORMANCE_DURATION)
    durationLabel:SetPoint("TOPLEFT", 17, -7)
    local duration = 10
    local durationButtons = {}
    local previousDuration
    for _, seconds in ipairs({ 10, 30, 60 }) do
        local button = ui.CreateButton(captureView, 72, string.format(L.PERFORMANCE_SECONDS, seconds), seconds == duration and "selected" or "ghost")
        if previousDuration then button:SetPoint("LEFT", previousDuration, "RIGHT", 2, 0)
        else button:SetPoint("TOPLEFT", 92, 0) end
        button:SetScript("OnClick", function()
            duration = seconds
            for value, durationButton in pairs(durationButtons) do
                ui.SetButtonVariant(durationButton, value == seconds and "selected" or "ghost")
            end
        end)
        durationButtons[seconds] = button
        previousDuration = button
    end
    local captureButton = ui.CreateButton(captureView, 140, L.PERFORMANCE_START_CAPTURE, true)
    captureButton:SetPoint("TOPRIGHT", -14, 0)
    local exportCapture = ui.CreateButton(captureView, 92, L.PERFORMANCE_EXPORT, "ghost")
    exportCapture:SetPoint("RIGHT", captureButton, "LEFT", -8, 0)
    local enableDeep = ui.CreateButton(captureView, 168, L.PERFORMANCE_DEEP_RELOAD, "ghost")
    enableDeep:SetPoint("LEFT", previousDuration, "RIGHT", 10, 0)
    enableDeep:SetScript("OnClick", function()
        if SetCVar then SetCVar("scriptProfile", "1") end
        if ReloadUI then ReloadUI() end
    end)
    local sessionPanel = ui.CreatePanel(captureView, ui.editorR, ui.editorG, ui.editorB, 0.78)
    sessionPanel:SetPoint("TOPLEFT", 14, -66)
    sessionPanel:SetPoint("BOTTOMLEFT", 14, 14)
    sessionPanel:SetWidth(SESSION_LIST_WIDTH)
    local sessionLabel = ui.CreateSectionLabel(captureView, L.PERFORMANCE_SESSION_LIST)
    sessionLabel:SetPoint("BOTTOMLEFT", sessionPanel, "TOPLEFT", 3, 7)
    local sessionScroll = ui.CreateScrollArea(sessionPanel, 8, 8, 7, 8)
    local sessionContent = CreateFrame("Frame", nil, sessionScroll)
    sessionContent:SetSize(SESSION_LIST_WIDTH - 26, 1)
    sessionScroll:SetScrollChild(sessionContent)
    local sessionEmpty = sessionPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    sessionEmpty:SetPoint("TOP", 0, -24)
    sessionEmpty:SetText(L.PERFORMANCE_NO_CAPTURE)
    sessionEmpty:SetTextColor(1, 1, 1, 0.34)
    local timeline = ui.CreatePanel(captureView, ui.editorR, ui.editorG, ui.editorB, 0.78)
    timeline:SetPoint("TOPLEFT", sessionPanel, "TOPRIGHT", 12, 0)
    timeline:SetPoint("TOPRIGHT", -14, -66)
    timeline:SetHeight(150)
    local timelineLabel = timeline:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    timelineLabel:SetPoint("TOPLEFT", 10, -8)
    timelineLabel:SetText(L.PERFORMANCE_CAPTURE_READY)
    timelineLabel:SetTextColor(1, 1, 1, 0.46)
    local captureReport = ui.CreateTextArea(captureView, true)
    captureReport:SetPoint("TOPLEFT", timeline, "BOTTOMLEFT", 0, -42)
    captureReport:SetPoint("BOTTOMRIGHT", -14, 14)
    captureReport.editBox:SetWidth(ui.windowWidth - SESSION_LIST_WIDTH - 68)
    ui.SetReadOnlyText(captureReport, L.PERFORMANCE_NO_CAPTURE)
    local bars = {}
    for index = 1, 80 do
        local bar = timeline:CreateTexture(nil, "ARTWORK")
        bar:SetColorTexture(ui.accentR, ui.accentG, ui.accentB, 0.78)
        bar:SetWidth(6)
        bar:SetHeight(1)
        bar:SetPoint("BOTTOMLEFT", 10 + (index - 1) * 8, 10)
        bars[index] = bar
    end
    local sessionRows = {}
    local captureResultMode = "overview"
    local resultDefinitions = {
        { key = "overview", label = L.PERFORMANCE_RESULT_OVERVIEW, width = 72 },
        { key = "cpu", label = L.PERFORMANCE_RESULT_CPU, width = 112 },
        { key = "objects", label = L.PERFORMANCE_RESULT_OBJECTS, width = 88 },
        { key = "storage", label = L.PERFORMANCE_RESULT_STORAGE, width = 94 },
        { key = "full", label = L.PERFORMANCE_RESULT_FULL, width = 92 },
    }
    local resultTabs = {}
    local previousResultTab
    for index = 1, #resultDefinitions do
        local definition = resultDefinitions[index]
        local tab = ui.CreateViewTab(captureView, definition.label)
        ui.FitViewTab(tab, definition.width, 30)
        if previousResultTab then tab:SetPoint("LEFT", previousResultTab, "RIGHT", 8, 0)
        else tab:SetPoint("TOPLEFT", timeline, "BOTTOMLEFT", 0, -5) end
        resultTabs[definition.key] = tab
        previousResultTab = tab
    end

    local function RenderCaptureResult()
        local text = L.PERFORMANCE_NO_CAPTURE
        if selectedSession then
            if captureResultMode == "cpu" then
                text = FormatCPUReport(selectedSession)
            elseif captureResultMode == "objects" then
                text = FormatObjectSummary(selectedSession.objectSummary)
            elseif captureResultMode == "storage" then
                text = FormatStorageDeltaReport(selectedSession)
            elseif captureResultMode == "full" then
                text = FormatFullCaptureReport(selectedSession)
            else
                text = FormatCaptureReport(selectedSession)
            end
        elseif selectedHealthEntry and captureResultMode == "overview" then
            text = FormatHealthReport(healthSnapshot, selectedHealthEntry)
        end
        ui.SetReadOnlyText(captureReport, text)
        for key, tab in pairs(resultTabs) do
            tab:SetActive(key == captureResultMode)
            ui.SetButtonEnabled(tab, key == "overview" or selectedSession ~= nil)
        end
    end
    for index = 1, #resultDefinitions do
        local definition = resultDefinitions[index]
        resultTabs[definition.key]:SetScript("OnClick", function()
            captureResultMode = definition.key
            RenderCaptureResult()
        end)
    end

    local function GetSelectedSessions()
        local filtered = {}
        local list = ns.Performance.GetSessions()
        for index = 1, #list do
            if list[index].addon == selectedAddon then
                filtered[#filtered + 1] = list[index]
            end
        end
        return filtered
    end

    local function DrawTimeline(session)
        local samples = session and session.samples or {}
        local first = math.max(1, #samples - #bars + 1)
        local maximum = 0
        for index = first, #samples do maximum = math.max(maximum, samples[index].lastTime or 0) end
        maximum = math.max(0.01, maximum)
        for index = 1, #bars do
            local sample = samples[first + index - 1]
            bars[index]:SetShown(sample ~= nil)
            if sample then bars[index]:SetHeight(math.max(1, math.floor(112 * (sample.lastTime or 0) / maximum))) end
        end
        if session then
            timelineLabel:SetText(string.format(L.PERFORMANCE_TIMELINE_STATUS,
                session.addon, FormatMilliseconds(samples[#samples] and samples[#samples].lastTime),
                FormatMilliseconds(maximum)))
        else
            timelineLabel:SetText(L.PERFORMANCE_CAPTURE_READY)
        end
    end

    local function ApplySessionRow(row)
        ui.SetListRowState(row, row.session == selectedSession, row.isHovered)
    end
    local function SelectSession(session)
        selectedSession = session
        RenderCaptureResult()
        DrawTimeline(session)
        ui.SetButtonEnabled(exportCapture, session ~= nil)
        for index = 1, #sessionRows do ApplySessionRow(sessionRows[index]) end
    end
    local function CreateSessionRow(index)
        local row = ui.CreateListRow(sessionContent, SESSION_ROW_HEIGHT)
        row:SetWidth(SESSION_LIST_WIDTH - 26)
        local title = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        title:SetPoint("TOPLEFT", 10, -7)
        title:SetPoint("TOPRIGHT", -10, -7)
        title:SetJustifyH("LEFT")
        title:SetWordWrap(false)
        row.title = title
        local metadata = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        metadata:SetPoint("BOTTOMLEFT", 10, 6)
        metadata:SetPoint("BOTTOMRIGHT", -10, 6)
        metadata:SetJustifyH("LEFT")
        metadata:SetWordWrap(false)
        metadata:SetTextColor(1, 1, 1, 0.40)
        row.metadata = metadata
        row:SetScript("OnEnter", function(self) self.isHovered = true ApplySessionRow(self) end)
        row:SetScript("OnLeave", function(self) self.isHovered = nil ApplySessionRow(self) end)
        row:SetScript("OnClick", function(self) SelectSession(self.session) end)
        sessionRows[index] = row
        return row
    end
    local function RefreshSessions()
        local list = GetSelectedSessions()
        if selectedSession and selectedSession.addon ~= selectedAddon then
            selectedSession = nil
        end
        for index = 1, #sessionRows do sessionRows[index]:Hide() end
        for index = 1, #list do
            local session = list[index]
            local row = sessionRows[index] or CreateSessionRow(index)
            row.session = session
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((index - 1) * SESSION_ROW_HEIGHT))
            row.title:SetText(string.format(L.PERFORMANCE_SESSION_TITLE,
                date("%H:%M:%S", session.createdAt or time())))
            row.metadata:SetText(string.format(L.PERFORMANCE_SESSION_META,
                session.elapsed or 0, FormatMilliseconds(session.summary and session.summary.p95),
                session.summary and session.summary.sampleCount or 0))
            ApplySessionRow(row)
            row:Show()
        end
        sessionContent:SetHeight(math.max(1, #list * SESSION_ROW_HEIGHT))
        sessionScroll:UpdateScrollChildRect()
        sessionEmpty:SetShown(#list == 0)
        if not selectedSession and list[1] then
            SelectSession(list[1])
        elseif not selectedSession then
            RenderCaptureResult()
            DrawTimeline(nil)
            ui.SetButtonEnabled(exportCapture, false)
        end
    end
    sessionScroll.onVerticalScrollChanged = RefreshSessions

    local function CaptureChanged(eventName, session)
        if eventName == "sample" then
            DrawTimeline(session)
            SetStatus(string.format(L.PERFORMANCE_CAPTURING, session.addon,
                math.min(session.duration, GetTime() - session.startedAt), session.duration), false)
        elseif eventName == "finished" then
            ui.SetButtonText(captureButton, L.PERFORMANCE_START_CAPTURE)
            ui.SetButtonVariant(captureButton, "primary")
            selectedSession = session
            RefreshSessions()
            SelectSession(session)
            SetStatus(string.format(L.PERFORMANCE_CAPTURE_FINISHED, session.summary.sampleCount), false)
            if ui.EndCaptureMode then ui.EndCaptureMode(true) end
        end
    end
    captureButton:SetScript("OnClick", function()
        if ns.Performance.IsCapturing() then
            ns.Performance.StopCapture("manual")
            return
        end
        local succeeded, result = ns.Performance.StartCapture(selectedAddon, duration,
            nil, nil, CaptureChanged)
        if not succeeded then SetStatus(result, true) return end
        ui.SetButtonText(captureButton, L.PERFORMANCE_STOP_CAPTURE)
        ui.SetButtonVariant(captureButton, "danger")
        DrawTimeline(result)
        if ui.BeginCaptureMode then
            ui.BeginCaptureMode(function() ns.Performance.StopCapture("manual") end)
        end
    end)
    exportCapture:SetScript("OnClick", function()
        if selectedSession then
            ui.ExportText("performance_capture", selectedSession.addon,
                function() return ns.SerializeForExport(selectedSession) end,
                { addon = selectedSession.addon, sampleCount = #selectedSession.samples, evidence = "exact+derived+observed" })
        end
    end)

    local functionView = CreateFrame("Frame", nil, page)
    functionView:SetPoint("TOPLEFT", 0, -126)
    functionView:SetPoint("BOTTOMRIGHT")
    views.functionLab = functionView
    local functionPathLabel = ui.CreateSectionLabel(functionView, L.PERFORMANCE_FUNCTION_PATH)
    functionPathLabel:SetPoint("TOPLEFT", 17, -2)
    local iterationLabel = ui.CreateSectionLabel(functionView, L.PERFORMANCE_ITERATIONS)
    iterationLabel:SetPoint("TOPLEFT", 435, -2)
    local callModeLabel = ui.CreateSectionLabel(functionView, L.PERFORMANCE_CALL_MODE)
    callModeLabel:SetPoint("TOPLEFT", 511, -2)
    local functionPath = CreateLineInput(functionView, ui, "")
    functionPath:SetPoint("TOPLEFT", 14, -22)
    functionPath:SetSize(410, 30)
    local functionHint = functionPath:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    functionHint:SetPoint("LEFT", 10, 0)
    functionHint:SetText(L.PERFORMANCE_FUNCTION_HINT)
    functionHint:SetTextColor(1, 1, 1, 0.30)
    local iterationInput = CreateLineInput(functionView, ui, "20")
    iterationInput:SetPoint("LEFT", functionPath, "RIGHT", 8, 0)
    iterationInput:SetSize(68, 30)
    local asMethod = false
    local methodMode = ui.CreateButton(functionView, 148, L.PERFORMANCE_NO_ARGUMENTS, false)
    methodMode:SetPoint("LEFT", iterationInput, "RIGHT", 8, 0)
    methodMode:SetScript("OnClick", function()
        asMethod = not asMethod
        ui.SetButtonText(methodMode, asMethod and L.PERFORMANCE_AS_METHOD or L.PERFORMANCE_NO_ARGUMENTS)
        ui.SetButtonVariant(methodMode, asMethod and "selected" or "secondary")
    end)
    local runBenchmark = ui.CreateButton(functionView, 108, L.PERFORMANCE_RUN_BENCHMARK, true)
    runBenchmark:SetPoint("TOPRIGHT", -14, -22)
    local exportBenchmark = ui.CreateButton(functionView, 92, L.PERFORMANCE_EXPORT, "ghost")
    exportBenchmark:SetPoint("RIGHT", runBenchmark, "LEFT", -8, 0)
    local warning = functionView:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    warning:SetPoint("TOPLEFT", 17, -64)
    warning:SetPoint("RIGHT", -17, 0)
    warning:SetJustifyH("LEFT")
    warning:SetText(L.PERFORMANCE_BENCHMARK_WARNING)
    warning:SetTextColor(1, 0.72, 0.32, 0.78)
    local benchmarkReport = ui.CreateTextArea(functionView, true)
    benchmarkReport:SetPoint("TOPLEFT", 14, -92)
    benchmarkReport:SetPoint("BOTTOMRIGHT", -14, 14)
    benchmarkReport.editBox:SetWidth(ui.windowWidth - 56)
    ui.SetReadOnlyText(benchmarkReport, L.PERFORMANCE_BENCHMARK_READY)
    functionPath.editBox:SetScript("OnTextChanged", function(self)
        functionHint:SetShown(self:GetText() == "")
    end)
    functionPath.editBox:SetScript("OnEditFocusGained", function()
        functionHint:Hide()
        ui.SetBorderColor(functionPath, true, 0.75)
    end)
    functionPath.editBox:SetScript("OnEditFocusLost", function(self)
        functionHint:SetShown(self:GetText() == "")
        ui.SetBorderColor(functionPath, false)
    end)
    runBenchmark:SetScript("OnClick", function()
        local succeeded, result, errorMessage = ns.Performance.RunBenchmark(
            functionPath.editBox:GetText(), iterationInput.editBox:GetText(), asMethod)
        if not succeeded then SetStatus(errorMessage, true) return end
        benchmarkResult = result
        result.addon = selectedAddon
        ui.SetReadOnlyText(benchmarkReport, FormatBenchmarkReport(result))
        ui.SetButtonEnabled(exportBenchmark, true)
        SetStatus(string.format(L.PERFORMANCE_BENCHMARK_DONE, result.iterations), false)
    end)
    exportBenchmark:SetScript("OnClick", function()
        if benchmarkResult then
            ui.ExportText("performance_benchmark", benchmarkResult.path,
                function() return ns.SerializeForExport(benchmarkResult) end,
                { addon = selectedAddon, path = benchmarkResult.path,
                    iterations = benchmarkResult.iterations, evidence = "exact" })
        end
    end)

    SetMode = function(newMode)
        mode = newMode
        for key, view in pairs(views) do view:SetShown(key == mode) end
        for key, tab in pairs(modeTabs) do tab:SetActive(key == mode) end
        status:Hide()
        changeAddon:SetShown(mode ~= "health" and selectedAddon ~= nil)
        if mode == "health" and not healthSnapshot then RefreshHealth() end
        if mode == "capture" then RefreshSessions() end
        if mode == "functionLab" and not benchmarkResult then
            ui.SetReadOnlyText(benchmarkReport, L.PERFORMANCE_BENCHMARK_READY)
            ui.SetButtonEnabled(exportBenchmark, false)
        end
    end
    for index = 1, #modes do
        local definition = modes[index]
        modeTabs[definition.key]:SetScript("OnClick", function() SetMode(definition.key) end)
    end
    changeAddon:SetScript("OnClick", function() SetMode("health") end)

    function page:Activate()
        SetMode(mode)
    end
    function page:Stop()
        if ns.Performance.IsCapturing() then ns.Performance.StopCapture("window_closed") end
        if ui.EndCaptureMode then ui.EndCaptureMode(false) end
    end

    page.modeTabs = modeTabs
    page.views = views
    page.refreshHealth = refreshHealth
    page.healthSearch = healthSearch
    page.captureButton = captureButton
    page.functionPath = functionPath
    page.runBenchmark = runBenchmark
    page.enableDeep = enableDeep
    page.resultTabs = resultTabs
    page.changeAddon = changeAddon
    page.healthRows = healthRows
    page.sessionRows = sessionRows
    page.healthReport = healthReport
    page.captureReport = captureReport
    page.benchmarkReport = benchmarkReport

    for key, view in pairs(views) do view:SetShown(key == mode) end
    for key, tab in pairs(modeTabs) do tab:SetActive(key == mode) end
    for key, tab in pairs(modeTabs) do
        if key ~= "health" then ui.SetButtonEnabled(tab, false) end
    end
    ui.SetButtonEnabled(exportHealth, false)
    ui.SetButtonEnabled(exportCapture, false)
    ui.SetButtonEnabled(exportBenchmark, false)
    enableDeep:SetShown(not (GetCVarBool and GetCVarBool("scriptProfile")))
    SetStatus(L.READY, false)
    return page
end
