local ADDON_NAME, ns = ...

-- Virtualized value-tree view: 22 px rows from a 32-row pool, expand/collapse,
-- bounded "load more" rows, right-click context for exportable nodes and
-- per-kind value colors.
local TREE_ROW_HEIGHT = 22
local TREE_ROW_POOL_SIZE = 32
local VALUE_COLUMN_LEFT = 250
local KEY_WIDTH_BASE = 218
local INDENT_WIDTH = 15

local function ValueColor(kind)
    if kind == "string" then
        return 0.62, 0.78, 0.64
    elseif kind == "number" then
        return 0.65, 0.74, 0.91
    elseif kind == "boolean" then
        return 0.90, 0.63, 0.42
    elseif kind == "table" then
        return 0.62, 0.67, 0.72
    elseif kind == "load_more" then
        return 0.86, 0.47, 0.55
    elseif kind == "marker" then
        return 0.85, 0.42, 0.48
    end
    return 0.70, 0.73, 0.76
end

ns.TreeView = {}

-- options: { contentWidth = number }
function ns.TreeView.Create(parent, options)
    options = type(options) == "table" and options or {}
    local W = ns.Widgets
    local colors = W.Colors
    local contentWidth = options.contentWidth or 500

    local panel = W.CreatePanel(parent, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    local scroll = W.CreateScrollArea(panel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(contentWidth)
    content:SetHeight(1)
    scroll:SetScrollChild(content)

    local empty = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(ns.L.TREE_EMPTY)
    empty:SetTextColor(1, 1, 1, 0.32)

    local rows = {}
    local visibleNodes = {}
    local tree
    local selectedNode
    local selectionChanged
    local nodeContext
    local view = { panel = panel }

    local function FlattenNodes(nodes, depth, visible)
        for index = 1, #nodes do
            local node = nodes[index]
            visible[#visible + 1] = { node = node, depth = depth }
            if node.kind == "table" and node.expanded and node.children then
                FlattenNodes(node.children, depth + 1, visible)
            end
        end
    end

    local function ApplyRowStyle(row)
        if row.node == selectedNode then
            row.hover:SetColorTexture(colors.accent[1], colors.accent[2], colors.accent[3], 0.16)
        elseif row.isHovered then
            row.hover:SetColorTexture(colors.surface[1], colors.surface[2], colors.surface[3], 0.72)
        else
            row.hover:SetColorTexture(0, 0, 0, 0)
        end
    end

    local function CreateRow(index)
        local row = CreateFrame("Button", nil, content)
        row:SetSize(contentWidth, TREE_ROW_HEIGHT)
        row:RegisterForClicks("LeftButtonUp", "RightButtonUp")

        local hover = row:CreateTexture(nil, "BACKGROUND")
        hover:SetAllPoints()
        hover:SetColorTexture(0, 0, 0, 0)
        row.hover = hover

        local toggle = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        toggle:SetJustifyH("CENTER")
        toggle:SetTextColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.95)
        row.toggle = toggle

        local key = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        key:SetJustifyH("LEFT")
        key:SetTextColor(0.91, 0.93, 0.95, 0.92)
        row.key = key

        local value = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        value:SetPoint("LEFT", VALUE_COLUMN_LEFT, 0)
        value:SetPoint("RIGHT", -8, 0)
        value:SetJustifyH("LEFT")
        value:SetWordWrap(false)
        row.value = value

        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplyRowStyle(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplyRowStyle(self)
        end)
        row:SetScript("OnClick", function(self, mouseButton)
            local node = self.node
            if not node then
                return
            elseif mouseButton ~= "RightButton" and node.kind == "load_more" and node.owner then
                ns.Inspector.LoadMore(node.owner)
                view:Refresh()
                return
            end

            if node.exportable then
                selectedNode = node
                if selectionChanged then
                    selectionChanged(node)
                end
            end
            if mouseButton == "RightButton" then
                if node.exportable and nodeContext then
                    nodeContext(node)
                end
                view:Refresh()
                return
            end
            if node.kind == "table" then
                if node.expanded then
                    node.expanded = false
                else
                    if not node.loaded then
                        ns.Inspector.LoadMore(node)
                    end
                    node.expanded = true
                end
            end
            view:Refresh()
        end)

        rows[index] = row
        return row
    end

    function view:HasTree()
        return tree and tree.roots and #tree.roots > 0
    end

    function view:GetSelectedNode()
        return selectedNode
    end

    function view:SetOnSelectionChanged(callback)
        selectionChanged = type(callback) == "function" and callback or nil
    end

    function view:SetOnNodeContext(callback)
        nodeContext = type(callback) == "function" and callback or nil
    end

    function view:SetTree(newTree)
        tree = newTree
        selectedNode = tree and tree.roots and tree.roots[1] and tree.roots[1].exportable
            and tree.roots[1] or nil
        scroll:SetVerticalScroll(0)
        self:Refresh()
        if selectionChanged then
            selectionChanged(selectedNode)
        end
    end

    function view:RenderRows()
        for index = 1, #rows do
            rows[index]:Hide()
        end

        local offset = scroll:GetVerticalScroll() or 0
        if issecretvalue and issecretvalue(offset) then
            return
        end
        local firstIndex = math.floor(math.max(0, offset) / TREE_ROW_HEIGHT) + 1
        local lastIndex = math.min(#visibleNodes, firstIndex + TREE_ROW_POOL_SIZE - 1)

        for visibleIndex = firstIndex, lastIndex do
            local poolIndex = visibleIndex - firstIndex + 1
            local item = visibleNodes[visibleIndex]
            local node = item.node
            local row = rows[poolIndex] or CreateRow(poolIndex)
            local indent = item.depth * INDENT_WIDTH
            row.node = node
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((visibleIndex - 1) * TREE_ROW_HEIGHT))

            row.toggle:ClearAllPoints()
            row.toggle:SetPoint("LEFT", 6 + indent, 0)
            row.toggle:SetSize(14, TREE_ROW_HEIGHT)
            if node.kind == "table" and (not node.loaded or #node.children > 0) then
                row.toggle:SetText(node.expanded and "-" or "+")
            elseif node.kind == "load_more" then
                row.toggle:SetText("+")
            else
                row.toggle:SetText("")
            end

            row.key:ClearAllPoints()
            row.key:SetPoint("LEFT", 24 + indent, 0)
            row.key:SetWidth(math.max(70, KEY_WIDTH_BASE - indent))
            row.key:SetText(node.label or "")

            local r, g, b = ValueColor(node.kind)
            row.value:SetTextColor(r, g, b, 0.92)
            row.value:SetText(node.value or "")
            ApplyRowStyle(row)
            row:Show()
        end
    end

    function view:Refresh()
        wipe(visibleNodes)
        if tree and tree.roots then
            FlattenNodes(tree.roots, 0, visibleNodes)
        end

        content:SetHeight(math.max(1, #visibleNodes * TREE_ROW_HEIGHT))
        scroll:UpdateScrollChildRect()
        empty:SetShown(#visibleNodes == 0)
        self:RenderRows()
    end

    scroll.onVerticalScrollChanged = function()
        view:RenderRows()
    end

    view.content = content
    view.rows = rows
    view.scroll = scroll
    return view
end
