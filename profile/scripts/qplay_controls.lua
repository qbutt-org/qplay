local executable = os.getenv("QPLAY_EXECUTABLE")
local configuration = os.getenv("QPLAY_CONFIG")
local data = os.getenv("QPLAY_DATA")
local ipc = os.getenv("QPLAY_IPC")
local i18n = dofile(mp.command_native({ "expand-path", "~~/script-modules/modernx/i18n.lua" }))

local function launch_control(option, callback)
    if not executable then
        if callback then
            callback(false)
        end
        return
    end
    local args = { executable, option }
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
    mp.command_native_async({ name = "subprocess", args = args, playback_only = false,
        capture_stdout = true, capture_stderr = true }, callback or function() end)
end

local function settings()
    launch_control("--settings")
end

mp.register_script_message("qplay-settings", settings)
mp.add_key_binding("Ctrl+,", "qplay-settings", settings)

local pending_position
local saving_position = false
local unload_hook

local function save_position()
    if saving_position then
        return
    end
    if not pending_position then
        if unload_hook then
            local hook = unload_hook
            unload_hook = nil
            hook:cont()
        end
        return
    end

    local value = pending_position
    pending_position = nil
    saving_position = true
    launch_control("--save-subtitle-position=" .. value, function(success, result)
        saving_position = false
        if not success or not result or result.status ~= 0 then
            mp.osd_message(i18n.get().subtitle_position_error)
            mp.msg.error(result and result.stderr or "Subtitle position save failed")
        end
        save_position()
    end)
end

mp.register_script_message("qplay-save-subtitle-position", function(value)
    local position = tonumber(value)
    if value ~= "auto" and (not position or position ~= position or position < 0 or position > 100) then
        return
    end
    pending_position = value
    save_position()
end)

mp.add_hook("on_unload", 50, function(hook)
    if saving_position or pending_position then
        hook:defer()
        unload_hook = hook
    end
end)
