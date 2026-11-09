-- Notes tool: two tools agents can call through Zeren's own MCP server
-- ("tools"; each is listed in the manifest). They run here, in this
-- plugin's sandbox, while Zeren is open on this Mac. `open_chat` says
-- which chat you're looking at, by its title ("sessions.read": without it
-- the session is only an id).

local zeren = require("./zeren")("notes-tool")

zeren.tool {
	name = "count_words",
	run = function(args: any): any
		local text = if type(args.text) == "string" then args.text else ""
		local words, lines = 0, 1
		for _ in string.gmatch(text, "%S+") do
			words += 1
		end
		for _ in string.gmatch(text, "\n") do
			lines += 1
		end
		return { words = words, lines = if text == "" then 0 else lines, characters = utf8.len(text) or #text }
	end,
}

zeren.tool {
	name = "open_chat",
	run = function(): any
		local session = zeren.context("session")
		if not session then
			return { open = false }
		end
		return { open = true, title = session.title or "Untitled" }
	end,
}
