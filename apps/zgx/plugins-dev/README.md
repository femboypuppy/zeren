# Example plugins

Seven plugins, each proving one thing Zeren's plugins can do. They are **not built in**: install one by copying its folder into a Zeren data dir's `plugins/`, then sign it or allow it (`install.sh` does both for a scratch engine).

| Folder | Proves | Asks for | Who signs it |
|---|---|---|---|
| `checkout-status` | A service (`service.luau`, run headless by the engine on each device): a session row's menu asks the service on that session's device for its checkout's branch and changes (running that device's `git`), and agents get the same through a tool with no window open | `ui.add`, `sessions.read`, `files.read`, `service`, `commands` (`git`), `tools` | you, installed and allowed on each device that should answer |
| `compact-sidebar` | Replacing a whole screen module (`ui.override` of `@app/screens/shell/sidebar`, wrapping `@app-original`) | `ui.replace` | you: loads once you've allowed `ui.replace` |
| `focus` | A title-bar button (`zeren.place` at `#titlebar-actions`) and a filtered sidebar (`sidebar.filter`) | `ui.add`, `ui.change` | another account: the consent sheet shows "Made by @…" |
| `notes-tool` | Luau tools agents call through Zeren's MCP server (`count_words`, `open_chat`) | `tools`, `sessions.read` | you |
| `ping` | A message between two of your devices, and back | `ui.add`, `devices` | you, installed on both devices |
| `session-status` | A node placed in each chat's header that reads its session (`zeren.need("session")`); placed where there's none, Settings says why | `ui.add`, `sessions.read` | you |
| `settings-demo` | Settings declared in its manifest, drawn by Zeren as its own page (Settings → Greeter), read with `zeren.setting(id)` and heard with `zeren.on_setting` | `ui.add` | you |

Each folder has its manifest (`zeron-plugin.json`), its `init.luau`, and a copy of Zeren's helpers (`zeren.luau`, from `apps/zgx/plugins-sdk/`), which `zeren plugin new` puts in every new plugin.

## What a plugin is

A plugin is a folder in `<data>/plugins/<id>/`:
- `zeron-plugin.json` holds its id, name, version, description, entry, the permissions it needs, its tools and its signature.
- Dropping a folder in installs it.

**Who made it.** The engine checks every plugin's signature against the account that owns the signing key.
- **Signed by you:** it loads once you've allowed what it asks. After that, edits re-sign it automatically and load without asking, until it asks for something new.
- **Signed by someone else:** it shows "Made by @username" and asks.
- **Unsigned, or changed since it was signed:** it shows "Unknown author". An unsigned plugin you've allowed asks again after any edit.
- **A program of its own for your agents** (`native`) always asks, even when it's yours.

**Signing your own.** Use `zeren plugin new <id>` (or New plugin in Settings → Plugins, or an agent's `create_plugin` tool), or `zeren plugin sign <id>`. Either way, your plugins stay yours as you and your agents edit them.

**Installing someone else's.**
- **The person:** Settings → Plugins → Install… takes a `.zeron-plugin` file or a link. The link can point to a package, a GitHub release that carries one, or a git repository (`owner/repo`, or `…/tree/<branch>/<folder>`).
- **An agent:** uses `install_plugin`.
- **Before it runs:** the signature is checked over every file first. Anyone else's waits until you allow what it asks; one signed by your own key turns on at once.
- **Where it came from** is kept in its folder's `.source.json`. Zeren checks there for updates, when Settings → Plugins opens and every few hours.
- **A row with an update** says what the update adds to or drops from what the plugin asks, and has Update.
- **An update must be signed by the same key**, or it's refused. One that asks for something new waits for you again.
- **Removing it** takes away its folder, its storage and everything it placed.

## Templates: Zeren's whole interface, yours

This needs no plugin. A template is a whole copy of Zeren's interface that you, or an agent, rewrite, like a WordPress theme. Any file a template leaves out is Zeren's own. Ask an agent, which uses Zeren's MCP tools:

| Tool | What it does |
|---|---|
| `describe_app` | What each screen draws. |
| `create_template { id, name, from? }` | A full copy of Zeren's interface (or of another template) to rewrite. It returns the folder, the files and a map of where things live. |
| `use_template { id }` | Switches the window to it, live. `zeren` is Zeren's own. |
| `template_status { id }` | Files with errors, and files out of date with Zeren's own. |
| `list_templates`, `delete_template { id }` | What's installed and in use; deleting the one in use switches back to Zeren's. |

- Each saved edit shows at once while that template is in use.
- A file with an error falls back to Zeren's own for that file, and `template_status` says why. If the template's `app.luau` fails, Zeren's own shows whole.
- Plugins work on top of any template through the elements it keeps (their ids and classes). One whose element a template dropped says "Not shown in this template".

## Writing one

```lua
local zeren = require("./zeren")("my-plugin")
zeren.place {
	at = "#titlebar-actions",
	after = function()
		return ui.button { label = "Hi", on_click = function() zeren.toast("Hello") end }
	end,
}
```

`zeren.luau`'s header lists everything, and the permission each part needs. A control in the title bar looks right as an island control: 24 points tall (`$zeren.island.control`) and quiet, its ink `$text.secondary`, washed with `$hover` under the pointer. Over a new chat's wallpaper the title bar gathers its glass around it. Plugins can't reach Zeren's design system's modules, but they can use its theme tokens (`$text.primary`, `$hover`, `$zeren.island.control`, …).

**Where it goes: selectors.** `zeren.place { at = <selector>, before | after | replace = <build> }` places a node at every element the selector matches: `#id`, `.class`, `#a >> .b` (a `.b` inside `#a`) or `role:button "Send"`. The build function runs once per match, as a component of its own. Agents get the full map from `describe_elements` (one of Zeren's MCP tools): each element's selector, where it is in words, what it gives to what's placed in it, and whether the window running now has it. The main ones:

| Where | Selectors | Gives |
|---|---|---|
| The window | `#shell` | `profile`, `appearance`, `route` (everywhere) |
| Sidebar | `#sidebar`, `.session-row`, `.session-title`, `.section-header`, `#sidebar-actions`, `#rail` | `sessions`; a row its `session`; a header its `project` |
| Title bar | `#titlebar`, `#titlebar-actions` | |
| Chat | `#chat-home`, `#chat-session`, `#session-header`, `#transcript`, `.message`, `.message-actions`, `#chat-composer`, `#composer-toolbar` | the session page its `session`; a message its `message` |
| Files | `#files-pane`, `#file-tree`, `#files-header`, `.file-row`, `.file-tab`, `#file-viewer` | a row, tab or the viewer its `file` |
| Side panel | `#panel`, `#panel-strip`, `.panel-tab`, `#panel-empty` | a tab its `panel` |

**Stable names.** These keep their names across Zeren's updates, and every template keeps them: `#sidebar`, `.session-row`, `#session-header`, `.message`, `#chat-composer`, `.file-row`, `.file-tab`, `.panel-tab`. Place by these where you can; others may move (`describe_elements` marks each).

Adding (`before`, `after`) needs `ui.add`. Anything marked `hook = false` (or private) is never reachable, whatever the selector: the account and sign-out, approvals, allowing plugins, deleting files, and the like.

**What it reads: ids, then records.** A place gives ids (`session = <id>`), and `zeren.context(name)` turns one into the record of it this plugin was allowed: fields it wasn't allowed are absent, never an error. `zeren.need(name)` is the same, except where there's none: the node draws nothing there, and Settings → Plugins says why ("Session status needs a session, but it's in the title bar").

| Grant | Adds |
|---|---|
| `sessions.read` | a session's title, status, project, device and times |
| `messages.read` | a message's role and text, in the open chat |
| `profile.read` | your id, name, username and avatar |
| `account.email` | your email (never while you hide email addresses) |
| `files.read` | a file's path and name; without it a file is an id of the plugin's own, never its path |

A node that replaces an element stands where it stood, so it reads what's given *above* that element. Zeren gives each place's context on what holds the element (a thin wrapper), never on the element itself: keep to that in a template, or a plugin replacing the element loses what it reads.

**Changing what's there** (`replace`, or `ui.filter`) needs `ui.change`, with each selector listed in the manifest's `ui.change`.
- The sidebar's sessions are one virtual list, so they're filtered by setting `ui.shared("sidebar.filter")` to `{ statuses = {…}, by = "<your plugin's name>" }`.
- That takes `ui.change` on `#sidebar` (or `.session-row`).

**Zeren's components** (no grant). Zeren lends every plugin its design system (zgx's `Capability::Package("ds")`, `plugins::lend_design_system`): a plugin requires `@ds/…` (`require("@ds/button")`) into its own VM, from the app running now (`.live`), so its buttons, badges and text match whatever template is in use. It lends components only, no data and no authority. `docs/plugins.md` ("Zeren's components") has the details; GitHub (`plugins-builtin/github`) is built on them.

**GitHub, through Zeren's sign-in** (`github`; `github.write` to comment). The plugin never holds a token. `zeren.github.watch_pr(session_id)` keeps a session's pull request watched (the engine pushes changes), and `zeren.github.pr(url)` / `zeren.github.diff(url)` answer for a pull request's own address on GitHub.

**Its settings** (no grant). The manifest's `settings` lists what the person can set (switch, choice, text, number, shortcut, folder; `docs/plugins.md` has the shape). Zeren draws them as the plugin's own page in Settings, under Plugins, while it's on, and finds them in Settings search; the plugin's row in Settings → Plugins links there. `zeren.setting(id):get()` reads one, `zeren.on_setting(fn)` hears each change; values are kept for that plugin alone (`.storage/<id>.settings.json`), defaults come from the manifest, and removing the plugin removes them.

**Programs it runs** (`commands`, with the manifest's `"commands": ["gh"]`). `zeren.run(program, args, { cwd, stdin, timeout })` runs one the manifest names, as you, on this Mac, with no shell; the consent sheet names each program. `docs/plugins.md` ("Programs and a database") has the details; GitHub (`plugins-builtin/github`) runs `gh`.

**Its own database** (no grant). `zeren.db.exec(sql, params)` and `zeren.db.query(sql, params)`: a SQLite file of its own (`.storage/<id>.sqlite3`, 100 MB at most, removed with the plugin), parameters always bound.

**What a plugin keeps** (no grant). `zeren.store(key, default)` keeps up to 1 MB between runs, for that plugin alone; removing the plugin removes it. `zeren.every(seconds, fn)` runs something now and then, at most every 30 seconds.

**Zeren's own plugins** ship inside the app (`plugins-builtin/`, e.g. GitHub). Each is allowed what it asks, which shows on its row in Settings → Plugins. They're on unless you turn them off, and turning one off removes everything it placed, at once. They can't be removed or edited. A folder of yours with the same id gives way.

**Replacing a screen** (`ui.override` of an `@app/screens/…` module) needs `ui.replace`, with each module listed in `ui.replace`. A replacement runs as part of Zeren and can do anything Zeren can, so the consent sheet says so.
