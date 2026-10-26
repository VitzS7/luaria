-- Lote B: Filesystem, Encoding, Drawing, Misc, Reflection.
-- Every file created here uses the sunc_own_ prefix so the cleanup at the end can remove it.

local FILE_CLEANUP = {
	"sunc_own_rw.txt", "sunc_own_app.txt", "sunc_own_del.txt", "sunc_own_is.txt",
	"sunc_own_list.txt", "sunc_own_lf.lua", "sunc_own_lf_bad.lua", "sunc_own_asset.txt",
}
local FOLDER_CLEANUP = {
	"sunc_own_dir_a", "sunc_own_dir_b", "sunc_own_mk", "sunc_own_dirdel", "sunc_own_list_dir",
}
local BLOCKED_EXTENSIONS = { ".exe", ".bat", ".ps1", ".js", ".xml", ".zip", ".url" }

-- why: hidden property names differ between builds, so probe a few candidates instead of hardcoding one
local HIDDEN_CANDIDATES = { "ReceiveAge", "UniqueId", "AvatarPos" }

local function findHiddenProperty(instance)
	for _, propName in ipairs(HIDDEN_CANDIDATES) do
		local ok, _, hidden = pcall(gethiddenproperty, instance, propName)
		if ok and hidden == true then
			return propName
		end
	end
	return nil
end

T("writefile", function()
	assert(exists("writefile"), "does not exist")
	writefile("sunc_own_rw.txt", "first")
	eq(readfile("sunc_own_rw.txt"), "first", "writefile then readfile")
	writefile("sunc_own_rw.txt", "second")
	eq(readfile("sunc_own_rw.txt"), "second", "overwrite")
	for _, ext in ipairs(BLOCKED_EXTENSIONS) do
		assert(expectError(writefile, "sunc_own_blocked" .. ext, "x"), "blocked extension accepted: " .. ext)
	end
	return true
end)

T("readfile", function()
	assert(exists("readfile"), "does not exist")
	writefile("sunc_own_rw.txt", "roundtrip")
	eq(readfile("sunc_own_rw.txt"), "roundtrip", "readfile roundtrip")
	assert(expectError(readfile, "sunc_own_missing_file.txt"), "readfile on missing file did not error")
	return true
end)

T("appendfile", function()
	assert(exists("appendfile"), "does not exist")
	if isfile("sunc_own_app.txt") then
		delfile("sunc_own_app.txt")
	end
	appendfile("sunc_own_app.txt", "a")
	appendfile("sunc_own_app.txt", "b")
	eq(readfile("sunc_own_app.txt"), "ab", "appendfile concatenates")
	for _, ext in ipairs(BLOCKED_EXTENSIONS) do
		assert(expectError(appendfile, "sunc_own_blocked" .. ext, "x"), "blocked extension accepted: " .. ext)
	end
	return true
end)

T("delfile", function()
	assert(exists("delfile"), "does not exist")
	writefile("sunc_own_del.txt", "x")
	assert(isfile("sunc_own_del.txt"), "file missing before delfile")
	delfile("sunc_own_del.txt")
	eq(isfile("sunc_own_del.txt"), false, "file still there after delfile")
	return true
end)

T("delfolder", function()
	assert(exists("delfolder"), "does not exist")
	makefolder("sunc_own_dirdel")
	delfolder("sunc_own_dirdel")
	eq(isfolder("sunc_own_dirdel"), false, "folder still there after delfolder")
	return true
end)

T("isfile", function()
	assert(exists("isfile"), "does not exist")
	writefile("sunc_own_is.txt", "x")
	eq(isfile("sunc_own_is.txt"), true, "isfile on file")
	makefolder("sunc_own_dir_a")
	eq(isfile("sunc_own_dir_a"), false, "isfile on folder")
	eq(isfile("sunc_own_does_not_exist.txt"), false, "isfile on missing path")
	return true
end)

T("isfolder", function()
	assert(exists("isfolder"), "does not exist")
	writefile("sunc_own_is.txt", "x")
	eq(isfolder("sunc_own_is.txt"), false, "isfolder on file")
	makefolder("sunc_own_dir_b")
	eq(isfolder("sunc_own_dir_b"), true, "isfolder on folder")
	return true
end)

T("listfiles", function()
	assert(exists("listfiles"), "does not exist")
	writefile("sunc_own_list.txt", "x")
	makefolder("sunc_own_list_dir")
	local entries = listfiles("")
	assert(type(entries) == "table", "listfiles did not return a table")
	local foundFile, foundFolder = false, false
	for _, entry in ipairs(entries) do
		-- entries may be bare names or full paths, so match by substring
		if string.find(entry, "sunc_own_list.txt", 1, true) then
			foundFile = true
		end
		if string.find(entry, "sunc_own_list_dir", 1, true) then
			foundFolder = true
		end
	end
	eq(foundFile, true, "listfiles misses created file")
	eq(foundFolder, true, "listfiles misses created folder")
	return true
end)

T("loadfile", function()
	assert(exists("loadfile"), "does not exist")
	writefile("sunc_own_lf.lua", "return ...")
	local chunk = loadfile("sunc_own_lf.lua")
	assert(type(chunk) == "function", "loadfile did not return a function")
	local first, second = chunk(7, 8)
	eq(first, 7, "varargs first")
	eq(second, 8, "varargs second")

	writefile("sunc_own_lf_bad.lua", "return (((")
	-- either an error or nil as first return is acceptable for a compile failure
	local ok, badChunk = pcall(loadfile, "sunc_own_lf_bad.lua")
	assert(not ok or badChunk == nil, "loadfile returned a function for invalid source")
	return true
end)

T("makefolder", function()
	assert(exists("makefolder"), "does not exist")
	makefolder("sunc_own_mk")
	makefolder("sunc_own_mk")
	eq(isfolder("sunc_own_mk"), true, "makefolder twice must keep the folder")
	return true
end)

T("getcustomasset", function()
	assert(exists("getcustomasset"), "does not exist")
	writefile("sunc_own_asset.txt", "content")
	local url = getcustomasset("sunc_own_asset.txt")
	assert(type(url) == "string", "getcustomasset did not return a string")
	eq(string.sub(url, 1, 11), "rbxasset://", "getcustomasset prefix")
	return true
end)

T("base64encode", function()
	assert(exists("base64encode"), "does not exist")
	eq(base64encode("DummyString\0\2"), "RHVtbXlTdHJpbmcAAg==", "base64encode known vector")
	return true
end)

T("base64decode", function()
	assert(exists("base64decode"), "does not exist")
	eq(base64decode("RHVtbXlTdHJpbmcAAg=="), "DummyString\0\2", "base64decode known vector")
	local binary = string.char(0, 1, 2, 255, 128, 10, 0)
	eq(base64decode(base64encode(binary)), binary, "binary roundtrip")
	return true
end)

T("lz4compress", function()
	assert(exists("lz4compress"), "does not exist")
	local input = string.rep("abcabc", 200)
	local compressed = lz4compress(input)
	assert(type(compressed) == "string", "lz4compress did not return a string")
	assert(#compressed < #input, "repetitive input was not reduced")
	return true
end)

T("lz4decompress", function()
	assert(exists("lz4decompress"), "does not exist")
	local input = string.rep("abcabc", 200)
	-- size is passed as second arg; sUNC-style lz4decompress needs it
	eq(lz4decompress(lz4compress(input), #input), input, "lz4 roundtrip")
	return true
end)

T("isrenderobj", function()
	assert(exists("isrenderobj"), "does not exist")
	local square = Drawing.new("Square")
	eq(isrenderobj(square), true, "Drawing object not recognized")
	eq(isrenderobj({}), false, "plain table recognized as render object")
	eq(isrenderobj("text"), false, "string recognized as render object")
	return true
end)

T("setrenderproperty", function()
	assert(exists("setrenderproperty"), "does not exist")
	local circle = Drawing.new("Circle")
	setrenderproperty(circle, "Radius", 50)
	eq(getrenderproperty(circle, "Radius"), 50, "Radius not stored")
	setrenderproperty(circle, "Visible", false)
	eq(getrenderproperty(circle, "Visible"), false, "Visible not stored")
	return true
end)

T("getrenderproperty", function()
	assert(exists("getrenderproperty"), "does not exist")
	local circle = Drawing.new("Circle")
	setrenderproperty(circle, "Radius", 12)
	eq(getrenderproperty(circle, "Radius"), 12, "getrenderproperty Radius")
	setrenderproperty(circle, "Visible", true)
	eq(getrenderproperty(circle, "Visible"), true, "getrenderproperty Visible")
	return true
end)

T("cleardrawcache", function()
	assert(exists("cleardrawcache"), "does not exist")
	local square = Drawing.new("Square")
	cleardrawcache()
	eq(square.__OBJECT_EXISTS, false, "cleardrawcache did not invalidate object")
	return true
end)

T("identifyexecutor", function()
	assert(exists("identifyexecutor"), "does not exist")
	local name, version = identifyexecutor()
	assert(type(name) == "string" and name ~= "", "executor name empty or not a string")
	assert(type(version) == "string" and version ~= "", "executor version empty or not a string")
	return true
end)

T("request", function()
	assert(exists("request"), "does not exist")
	SKIP("needs a local server")
end)

T("gethiddenproperty", function()
	assert(exists("gethiddenproperty"), "does not exist")
	local part = Instance.new("Part")
	local value, hidden = gethiddenproperty(part, "Name")
	eq(value, "Part", "Name value")
	eq(hidden, false, "Name must not be reported hidden")
	local hiddenName = findHiddenProperty(part)
	assert(hiddenName ~= nil, "no hidden property found among candidates; adjust HIDDEN_CANDIDATES")
	local _, hiddenFlag = gethiddenproperty(part, hiddenName)
	eq(hiddenFlag, true, "hidden property not reported as hidden")
	return true
end)

T("sethiddenproperty", function()
	assert(exists("sethiddenproperty"), "does not exist")
	local part = Instance.new("Part")
	sethiddenproperty(part, "Name", "sunc_own_renamed")
	eq(part.Name, "sunc_own_renamed", "sethiddenproperty on visible property")
	local hiddenName = findHiddenProperty(part)
	assert(hiddenName ~= nil, "no hidden property found among candidates; adjust HIDDEN_CANDIDATES")
	local original = gethiddenproperty(part, hiddenName)
	sethiddenproperty(part, hiddenName, original)
	eq(gethiddenproperty(part, hiddenName), original, "hidden property roundtrip")
	return true
end)

T("getthreadidentity", function()
	assert(exists("getthreadidentity"), "does not exist")
	local identity = getthreadidentity()
	assert(type(identity) == "number", "getthreadidentity did not return a number")
	return true
end)

T("setthreadidentity", function()
	assert(exists("setthreadidentity"), "does not exist")
	local original = getthreadidentity()
	setthreadidentity(2)
	local current = getthreadidentity()
	setthreadidentity(original)
	eq(current, 2, "setthreadidentity(2) not reflected by getthreadidentity")
	return true
end)

T("isscriptable", function()
	assert(exists("isscriptable"), "does not exist")
	local part = Instance.new("Part")
	eq(isscriptable(part, "Name"), true, "Name should be scriptable")
	eq(isscriptable(part, "sunc_own_no_such_property"), nil, "missing property should return nil")
	return true
end)

T("setscriptable", function()
	assert(exists("setscriptable"), "does not exist")
	local part = Instance.new("Part")
	setscriptable(part, "Name", false)
	eq(isscriptable(part, "Name"), false, "setscriptable(false) not reflected")
	setscriptable(part, "Name", true)
	eq(isscriptable(part, "Name"), true, "setscriptable(true) not reflected")

	-- restore original state so this test does not leak into other checks
	local original = isscriptable(workspace, "SignalBehavior")
	setscriptable(workspace, "SignalBehavior", true)
	local readable = pcall(function()
		local _ = workspace.SignalBehavior
	end)
	if original ~= nil then
		setscriptable(workspace, "SignalBehavior", original)
	end
	assert(readable, "SignalBehavior still not readable after setscriptable(true)")
	return true
end)

CLEANUP(function()
	for _, name in ipairs(FILE_CLEANUP) do
		if isfile(name) then
			delfile(name)
		end
	end
	for _, name in ipairs(FOLDER_CLEANUP) do
		if isfolder(name) then
			delfolder(name)
		end
	end
end)
