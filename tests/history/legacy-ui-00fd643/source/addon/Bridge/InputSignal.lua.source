local _, ns = ...

-- Three physical 2x2 cells. This display owns no input or independent clock.
local frame,blocks,lastScale
local colors={red={1,0,0},green={0,1,0},blue={0,0,1},black={0,0,0},white={1,1,1}}
local codes={ready={"green","white"},input_keyboard_focus={"red","blue"},input_combat_lockdown={"blue","red"},unknown={"white","black"}}
local function create()
    if frame then return frame end
    frame=CreateFrame("Frame",nil,UIParent)
    frame:Hide();frame:EnableMouse(false);frame:SetFrameStrata("TOOLTIP");frame:SetFrameLevel(10000)
    blocks={}
    for i=1,3 do blocks[i]=frame:CreateTexture(nil,"ARTWORK") end
    return frame
end
ns.InputSignal={
    Frame=create,
    Hide=function()if blocks then for _,block in ipairs(blocks) do block:Hide() end end end,
    Render=function(state,heartbeat)
        local code=codes[state]
        if not frame or not code then return nil end
        local px=ns.Compat.GetPhysicalPixelSize()
        if (issecretvalue and issecretvalue(px)) or type(px)~="number" or px~=px or px<=0 or px==math.huge then return nil end
        if px~=lastScale then
            frame:ClearAllPoints();frame:SetPoint("TOPLEFT",UIParent,"TOPLEFT",0,0);frame:SetSize(6*px,2*px)
            for i,block in ipairs(blocks) do
                block:SetPoint("TOPLEFT",frame,"TOPLEFT",(i-1)*2*px,0);block:SetSize(2*px,2*px)
            end
            lastScale=px
        end
        for i=1,2 do local c=colors[code[i]];blocks[i]:SetColorTexture(c[1],c[2],c[3],1) end
        local value=heartbeat and 1 or 0
        blocks[3]:SetColorTexture(value,value,value,1)
        for _,block in ipairs(blocks) do block:Show() end
        frame:Show();return true
    end,
}
