-- About page: lazy construction, the metadata and runtime-environment rows
-- and the lazy, rewrite-guarded, pre-selected repository link popup.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
assert(ns.Persistence.Load(), "persistence did not load")
local W = ns.Workbench
local L = ns.L

assert(W.Toggle(), "workbench did not open")

local framesBeforeAbout = Env.framesCreated
LycheeToolkitWindow.pageTabs.about:Click()
assert(Env.framesCreated > framesBeforeAbout, "about page was not constructed on first activation")
assert(W.GetActivePage() == "about", "about tab did not activate")

local aboutPage = assert(W.GetPage("about"), "about page object was not built")
assert(aboutPage.metaItems, "about metadata rows were not created")

-- Version comes from TOC metadata with the release string as fallback.
assert(aboutPage.metaItems.version.value:GetText() == "2.0.3",
    "about page did not read the addon version")
assert(aboutPage.metaItems.command.value:GetText() == "/dev", "about open command changed")
assert(aboutPage.metaItems.author.value:GetText() == "Follen", "about author changed")

-- Supported clients get their own full-width row.
assert(aboutPage.metaItems.clients.value:GetWidth() == 920,
    "supported clients did not receive the full metadata width")
assert(aboutPage.metaItems.clients.value.point[3] ~= aboutPage.metaItems.command.value.point[3],
    "supported clients did not receive a dedicated metadata row")
assert(aboutPage.metaItems.clients.value:GetText() == L.ABOUT_CLIENT_VALUE,
    "supported clients summary changed")

-- The repository popup is created lazily and pre-selects its URL.
assert(rawget(aboutPage, "linkPopup") == nil,
    "about page created the repository popup before it was needed")
aboutPage.githubButton:Click()
assert(aboutPage.linkPopup and aboutPage.linkPopup:IsShown()
        and aboutPage.linkBackdrop:IsShown(),
    "GitHub button did not open the repository popup")
assert(aboutPage.urlBox:GetText() == "https://github.com/Follen/Lychee-Dev",
    "repository popup did not show the correct address")
assert(aboutPage.urlBox.focused and aboutPage.urlBox.highlighted,
    "repository popup did not pre-select its address")

-- The URL box is rewrite-guarded: editing snaps back to the saved address.
aboutPage.urlBox:SetText("tampered")
assert(aboutPage.urlBox:GetText() == "https://github.com/Follen/Lychee-Dev",
    "repository address box accepted edits")

-- Escape closes the popup.
Env.FireScript(aboutPage.urlBox, "OnEscapePressed")
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "Escape did not close the repository popup")

-- Clicking outside closes the popup.
aboutPage.githubButton:Click()
aboutPage.linkBackdrop:Click()
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "clicking outside did not close the repository popup")

-- Leaving the page closes the popup.
aboutPage.githubButton:Click()
assert(aboutPage.linkPopup:IsShown(), "repository popup did not reopen")
LycheeToolkitWindow.pageTabs.runner:Click()
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "leaving the About page did not close the repository popup")

-- Media is loaded from the addon itself.
local foundLogo, foundGitHub
for index = 1, #Env.textures do
    if Env.textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\Logo.png" then
        foundLogo = true
    elseif Env.textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\GitHub.png" then
        foundGitHub = true
    end
end
assert(foundLogo, "logo texture was not loaded from the addon")
assert(foundGitHub, "about page did not load the GitHub icon")

print("about ok")
