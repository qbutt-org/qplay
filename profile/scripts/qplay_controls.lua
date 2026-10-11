local executable = os.getenv("QPLAY_EXECUTABLE")
local configuration = os.getenv("QPLAY_CONFIG")
local data = os.getenv("QPLAY_DATA")
local ipc = os.getenv("QPLAY_IPC")

local function settings()
    if not executable then
        return
    end
    local args = { executable, "--settings" }
    if ipc then
        args[#args + 1] = "--player-ipc=" .. ipc
    end
    -- Explicit profile roots keep portable and isolated launches on the same settings.
    if configuration and data and configuration:match("[\\/]config$") then
        local root = configuration:gsub("[\\/]config$", "")
        if data == root .. "\\data" or data == root .. "/data" then
            args[#args + 1] = "--profile-dir=" .. root
        end
    end
    mp.command_native_async({ name = "subprocess", args = args, playback_only = false }, function() end)
end

mp.register_script_message("qplay-settings", settings)
mp.add_key_binding("Ctrl+,", "qplay-settings", settings)
