-- Hello blocks: how a renderer for a kind of code block is a plugin. A
-- reply's ```hello block is replaced ("ui.change" on `.md-code.lang-hello`)
-- by a card of its own, built from what the block gives: `code`, its
-- language and source. A Mermaid or chart renderer is the same with a
-- different `lang-` and a drawing in place of the card.
--
-- Each block is replaced on its own, and one still streaming in is built
-- again as its source grows. With the plugin off, the block is the code
-- block it was.

local zeren = require("./zeren")("md-hello")

zeren.place {
	at = ".md-code.lang-hello",
	replace = function(): Node
		local code = zeren.need("code")
		return ui.column {
			class = "md-hello",
			w_full = true,
			gap = "$space.1",
			p = "$space.4",
			rounded = "$radius.xl",
			bg = "$zeren.secondary.bg",
			border = 1,
			border_color = "$accent",
			ui.text { "Hello", text_size = "$font.size.xs", color = "$accent" },
			ui.text { code.source, literal = true, wrap = true, color = "$text.primary" },
		}
	end,
}
