local builds = {
    retail = {
        toc = "Lychee Dev_Mainline.toc",
        interface = "120100",
        flavor = "Mainline",
        client = "Core/Clients/Mainline.lua",
        catalog = "Modules/Events/CatalogData_Mainline.lua",
        id = "retail",
    },
    classic = {
        toc = "Lychee Dev_Mists.toc",
        interface = "50504",
        flavor = "Mists",
        client = "Core/Clients/Mists.lua",
        catalog = "Modules/Events/CatalogData_Mists.lua",
        id = "classic",
    },
    titan = {
        toc = "Lychee Dev_Wrath.toc",
        interface = "38002",
        flavor = "Titan",
        client = "Core/Clients/Titan.lua",
        catalog = "Modules/Events/CatalogData_Titan.lua",
        id = "titan",
    },
}

local function ReadToc(path)
    local file = assert(io.open(path, "rb"))
    local metadata = {}
    local files = {}
    for line in file:lines() do
        line = line:gsub("\r$", "")
        local key, value = line:match("^## ([^:]+):%s*(.-)%s*$")
        if key then
            metadata[key] = value
        elseif line ~= "" and not line:match("^#") then
            files[#files + 1] = line:gsub("\\", "/")
        end
    end
    file:close()
    return metadata, files
end

local sharedFiles
for name, build in pairs(builds) do
    local metadata, files = ReadToc(build.toc)
    assert(metadata.Interface == build.interface, name .. " Interface mismatch")
    assert(metadata["X-Flavor"] == build.flavor, name .. " flavor mismatch")
    assert(files[1] == build.client, name .. " client profile was not loaded first")
    assert(files[2] == "Core/Compatibility.lua", name .. " compatibility layer load order changed")

    local catalogIndex
    for index = 1, #files do
        if files[index]:match("^Modules/Events/CatalogData_") then
            assert(not catalogIndex, name .. " loads more than one event catalog")
            catalogIndex = index
        end
    end
    assert(catalogIndex and files[catalogIndex] == build.catalog,
        name .. " event catalog does not match the selected client")

    local chunk = assert(loadfile(build.client))
    local ns = {}
    chunk("Lychee Dev", ns)
    assert(ns.Client.id == build.id and tostring(ns.Client.interface) == build.interface,
        name .. " client profile metadata mismatch")

    for index = 1, #files do
        local file = assert(io.open(files[index], "rb"), name .. " TOC file missing: " .. files[index])
        file:close()
    end

    local normalizedFiles = {}
    for index = 2, #files do
        normalizedFiles[index - 1] = index == catalogIndex and "Modules/Events/CatalogData_<client>.lua"
            or files[index]
    end
    local currentShared = table.concat(normalizedFiles, "\n")
    if sharedFiles then
        assert(currentShared == sharedFiles, name .. " shared TOC load order diverged")
    else
        sharedFiles = currentShared
    end
end

assert(not io.open("Lychee Dev.toc", "rb"), "ambiguous legacy TOC still exists")
print("Lychee Dev build matrix tests passed")
