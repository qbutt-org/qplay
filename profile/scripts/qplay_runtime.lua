local utils = require "mp.utils"
local msg = require "mp.msg"
local i18n = dofile(mp.command_native({ "expand-path", "~~/script-modules/modernx/i18n.lua" }))
local runtime = os.getenv("QPLAY_MEDIA_RUNTIME")
local manager = mp.command_native({
    "expand-path", "~~/tools/bin/framegen-manager.exe",
})

if not runtime then
    return
end

mp.set_property_native("user-data/qplay-auto-framegen", os.getenv("QPLAY_FRAMEGEN") ~= "no")

mp.command_native_async({
    name = "subprocess",
    args = {
        "powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", mp.command_native({ "expand-path", "~~/tools/hardware/shader-profile.ps1" }),
    },
    playback_only = false,
    capture_stdout = true,
    capture_stderr = true,
}, function(success, result)
    if not success or not result or result.status ~= 0 then
        msg.warn(i18n.localize("Не удалось определить возможности FX; используется базовый профиль"))
        return
    end

    local profile = utils.parse_json(result.stdout or "")
    if type(profile) == "table" then
        mp.set_property_native("user-data/qplay-shader-profile", profile)
    end
end)

local function publish(status, reason)
    mp.set_property_native("user-data/qplay-runtime", {
        status = status,
        reason = reason,
    })
end

local function run(command, callback)
    mp.command_native_async({
        name = "subprocess",
        args = { manager, command },
        playback_only = false,
        capture_stdout = true,
        capture_stderr = true,
    }, function(success, result)
        if not success or not result or result.status ~= 0 then
            msg.error(string.format("framegen-manager %s failed (%s): %s", command,
                tostring(result and result.status or "launch"), result and result.stderr or ""))
            publish("error", "Не удалось подготовить генерацию кадров")
            return
        end
        callback(utils.parse_json(result.stdout or ""))
    end)
end

local manifest = utils.join_path(runtime, "runtime-manifest.json")
if utils.file_info(manifest) then
    publish("ready", "")
    return
end

local function prepare_runtime()
    local setup = mp.get_property_native("user-data/qplay-runtime", {})
    if setup.status == "checking" or setup.status == "preparing" or setup.status == "ready" then
        return
    end
    if mp.get_property_native("user-data/qplay-auto-framegen", true) ~= true
            and mp.get_property_native("user-data/framegen/enabled", false) ~= true then
        return
    end
    local video_selected = false
    for _, track in ipairs(mp.get_property_native("track-list", {})) do
        if track.type == "video" and track.selected and not track.image then
            video_selected = true
            break
        end
    end
    if not video_selected then
        return
    end
    publish("checking", "Проверка возможностей видеокарты")
    run("plan", function(plan)
        if not plan or not plan.gpu or not plan.gpu.supported then
            publish("unavailable", "Генерация кадров недоступна на этой видеокарте")
            return
        end

        publish("preparing", "Подготовка генерации кадров")
        mp.osd_message(i18n.localize("Подготовка генерации кадров… Видео можно смотреть сейчас."), 5)
        run("install", function(result)
            if not result or not result.status or result.status.state ~= "ready" then
                publish("error", "Не удалось подготовить генерацию кадров")
                return
            end
            publish("ready", "")
            mp.commandv("script-message", "framegen-refresh")
        end)
    end)
end

mp.register_event("file-loaded", prepare_runtime)
mp.observe_property("user-data/framegen/enabled", "bool", function(_, enabled)
    if enabled then
        prepare_runtime()
    end
end)
