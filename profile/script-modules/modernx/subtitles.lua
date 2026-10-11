local M = {}
local input = require("mp.input")
local i18n = dofile(mp.command_native({ "expand-path", "~~/script-modules/modernx/i18n.lua" }))
local section = "qplay-subtitle-position"
local position_property = "user-data/qplay-subtitle-position"
local moving = false
local dragging = false
local context
local rail
local text_codecs = { ass = true, ssa = true, subrip = true, srt = true, webvtt = true,
    mov_text = true, text = true, ttml = true }

local function selected_track()
    for _, track in ipairs(mp.get_property_native("track-list", {})) do
        if track.type == "sub" and track.selected then
            return track
        end
    end
end

function M.has_session()
    return type(mp.get_property_native("user-data/qbutt-subtitles")) == "table"
end

function M.status()
    local session = mp.get_property_native("user-data/qbutt-subtitles")
    if type(session) ~= "table" then
        return ""
    end
    local texts = i18n.get()
    if session.state == "translating" then
        return session.progress and session.progress.phase == "connecting"
            and texts.translation_connecting or texts.translation_working
    elseif session.state == "translated" then
        return texts.translation_ready
    elseif session.state == "error" then
        return texts.translation_error .. (session.message and session.message ~= "" and "\n" .. session.message or "")
    elseif session.state == "unsupported" then
        return texts.translation_unsupported
    elseif session.state == "no-track" then
        return texts.nosub
    end
    return ""
end

function M.is_moving()
    return moving
end

function M.has_position()
    return type(mp.get_property_native(position_property)) == "number"
end

local function save_position()
    mp.commandv("script-message", "qplay-save-subtitle-position", tostring(mp.get_property_number("sub-pos", 100)))
end

local function stop_moving()
    if dragging then
        save_position()
    end
    moving = false
    dragging = false
    mp.disable_key_bindings(section)
    context.request_tick()
end

local function move_to_mouse()
    local _, y = context.mouse_position()
    local position = math.floor(math.max(0, math.min(100, (y - rail.top) / rail.height * 100)) + 0.5)
    mp.set_property_native(position_property, position)
    mp.set_property_number("sub-pos", position)
    context.request_tick()
end

mp.set_key_bindings({
    { "MBTN_LEFT", function()
        if dragging then
            dragging = false
            save_position()
            context.request_tick()
        end
    end, function()
        local _, y = context.mouse_position()
        if y > rail.top + rail.height + 15 then
            stop_moving()
        else
            dragging = true
            move_to_mouse()
        end
    end },
    { "mouse_move", function()
        if dragging then
            move_to_mouse()
        end
    end },
    { "MBTN_LEFT_DBL", "ignore" },
    { "ESC", stop_moving },
}, section, "force")
mp.disable_key_bindings(section)

function M.open(ctx)
    if moving then
        stop_moving()
    end
    context = ctx
    local texts = i18n.get()
    local items, actions = {}, {}
    local selected = 1
    local function add(label, action, current)
        items[#items + 1] = label
        actions[#actions + 1] = action
        if current then
            selected = #items
        end
    end
    local function translation(action)
        mp.commandv("script-message", "qplay-subtitles", action)
    end
    local session = mp.get_property_native("user-data/qbutt-subtitles")
    if type(session) == "table" then
        if session.canTranslate and session.state ~= "translating" and session.state ~= "translated" then
            add(session.state == "error" and texts.retry_subtitles or texts.translate_subtitles, function()
                translation(session.state == "error" and "retry" or "translate")
            end)
        end
        if session.state == "translating" or session.state == "translated" then
            add(texts.original_subtitles, function() translation("original") end)
        end
    end

    local current = selected_track()
    for _, track in ipairs(mp.get_property_native("track-list", {})) do
        if track.type == "sub" then
            local id = track.id
            local label = track.title or (texts.subtitle .. " " .. id)
            if track.lang then
                label = label .. " [" .. track.lang .. "]"
            end
            add(label, function()
                mp.set_property("secondary-sid", "no")
                mp.set_property_number("sid", id)
                mp.set_property_bool("sub-visibility", true)
            end, track.selected and mp.get_property_bool("sub-visibility", true))
        end
    end
    add(texts.off, function() mp.set_property("sid", "no") end,
        not current or not mp.get_property_bool("sub-visibility", true))

    if current and text_codecs[current.codec] and mp.get_property_bool("sub-visibility", true) then
        add(texts.move_subtitles, function()
            moving = true
            ctx.request_tick()
        end)
    end
    if M.has_position() then
        add(texts.reset_subtitle_position, function()
            mp.del_property(position_property)
            mp.set_property_number("sub-pos", 100)
            ctx.adjust_subtitles(true)
            mp.commandv("script-message", "qplay-save-subtitle-position", "auto")
            ctx.request_tick()
        end)
    end
    if type(session) == "table" then
        add(texts.translation_settings, function() translation("settings") end)
    end
    input.select({ prompt = texts.subtitle, items = items,
        default_item = selected, submit = function(index) actions[index]() end })
end

function M.draw(ass, ctx)
    if not moving then
        return
    end
    local track = selected_track()
    if not track or not text_codecs[track.codec] then
        stop_moving()
        return
    end
    rail = { x = ctx.width - 52, top = 104, height = math.max(32, math.min(260, ctx.height - 280)) }
    if dragging then
        ctx.set_mouse_area(0, 0, ctx.width, ctx.height, section)
    else
        ctx.set_mouse_area(rail.x - 36, rail.top - 20, rail.x + 34, rail.top + rail.height + 52, section)
    end
    mp.enable_key_bindings(section)

    local function box(x0, y0, x1, y1, color)
        ass:new_event()
        ass:append("{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H" .. color .. "&}")
        ass:draw_start()
        ass:rect_cw(x0, y0, x1, y1)
        ass:draw_stop()
    end
    local function text(x, y, value, size)
        ass:new_event()
        ass:append(string.format("{\\an9\\pos(%f,%f)\\fnSegoe UI\\fs%d\\bord1\\shad0\\1c&HFFFFFF&}%s", x, y, size, value))
    end
    local texts = i18n.get()
    text(ctx.width - 20, 48, texts.subtitle_position, 20)
    if track.codec == "ass" or track.codec == "ssa" then
        text(ctx.width - 20, 74, texts.positioned_ass, 14)
    end
    box(rail.x - 2, rail.top, rail.x + 2, rail.top + rail.height, "706860")
    local position = mp.get_property_number("sub-pos", 100)
    local y = rail.top + rail.height * math.max(0, math.min(100, position)) / 100
    box(rail.x - 13, y - 5, rail.x + 13, y + 5, "F79D00")
    text(rail.x - 20, y - 12, string.format("%.0f%%", position), 17)
    text(rail.x + 28, rail.top + rail.height + 23, texts.position_done, 18)
end

mp.add_hook("on_unload", 40, function()
    if moving then
        stop_moving()
    end
end)
mp.register_event("file-loaded", function()
    local position = mp.get_property_native(position_property)
    if type(position) == "number" then
        mp.set_property_number("sub-pos", position)
    end
end)
if os.getenv("QPLAY_SUBTITLE_POSITION") == "saved" then
    mp.set_property_native(position_property, mp.get_property_number("sub-pos", 100))
end

return M
