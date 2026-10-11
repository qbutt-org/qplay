local M = {}

function M.add_right_controls(ctx)
    local right_edge = ctx.osc_geo.w - 25
    local positions = {}

    local function add_button(name, width, style)
        width = width or 24
        local x = right_edge - width / 2
        right_edge = right_edge - width - 21

        local lo = ctx.add_layout(name)
        lo.geometry = { x = x, y = ctx.ref_y - 40, an = 5, w = width, h = 24 }
        lo.style = style or ctx.style
        positions[name] = x
    end

    add_button("tog_fs")
    add_button("qplay_settings", 56, ctx.style .. "{\\fnSegoe UI\\fs18}")

    if ctx.loop_button then
        add_button("tog_loop")
    end

    if ctx.ontop_button then
        add_button("tog_ontop")
    end

    if ctx.shader_preset_button then
        add_button("shader_preset")
    end

    if ctx.info_button then
        add_button("tog_info")
    end

    if ctx.screenshot_button then
        add_button("screenshot")
    end

    if ctx.download_button then
        add_button("download")
    end

    return positions
end

return M
