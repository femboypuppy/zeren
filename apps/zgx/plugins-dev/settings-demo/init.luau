-- Greeter: a greeting in the title bar ("ui.add", at #titlebar-actions),
-- set up on its own page in Settings (zeron-plugin.json's "settings":
-- Zeren draws the page and keeps what you set, for this plugin alone).
-- It reads its settings as they change (`zeren.setting(id):get()`), and
-- hears each change (`zeren.on_setting`), here said in a toast.

local zeren = require("./zeren")("settings-demo")

local show = zeren.setting("show")
local words = zeren.setting("words")
local tone = zeren.setting("tone")

zeren.on_setting(function(id: string, value: any, _was: any)
	zeren.toast(`Greeter: {id} is now {tostring(value)}`)
end)

-- What Zeren says beside the Greeting: a warning while it says nothing.
local function check_words()
	local said = words:peek() or ""
	if string.match(said, "^%s*$") then
		zeren.set_status("words", { badge = "Empty", tone = "warning", text = "Say something, or it greets with nothing." })
	else
		zeren.set_status("words", nil)
	end
end
check_words()
zeren.on_setting(function(id: string)
	if id == "words" then
		check_words()
	end
end)

zeren.place {
	at = "#titlebar-actions",
	after = function(): Node
		return ui.text {
			id = "greeter",
			function(): string
				if not show:get() then
					return ""
				end
				local said = words:get() or ""
				return if tone:get() == "excited" then `{said}!` else said
			end,
			text_size = "$font.size.sm",
			color = "$text.secondary",
		}
	end,
}
