-- Session words: session-status (a word in a chat's header for what its
-- session is doing), in Zeren's language. Its words are its own,
-- locales/<locale>.json (en.json the source), shown with zeren.t: they
-- follow Settings → General → Language as it changes, and a language the
-- plugin lacks falls back to its English.

local zeren = require("./zeren")("session-words")

local KEYS: { [string]: string } = {
	working = "status.working",
	input = "status.input",
	approval = "status.approval",
	failed = "status.failed",
	done = "status.done",
}

zeren.place {
	at = "#page-title",
	after = function(): Node
		local id = zeren.need("session").id
		return ui.text {
			id = "session-words",
			function(): string
				local key = KEYS[zeren.context_of("session", id).status or ""]
				return if key then zeren.s(key) else ""
			end,
			text_size = "$font.size.sm",
			color = "$text.secondary",
		}
	end,
}
