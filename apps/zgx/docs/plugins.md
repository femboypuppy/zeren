# Writing a Zeren plugin

A plugin is a folder: `zeren-plugin.json` (its manifest), `init.luau` (its code) and `zeren.luau` (Zeren's helpers, the SDK). It runs in Zeren's window in a sandbox of its own, with only what its manifest asks for and the person allowed. Agents make plugins through Zeren's MCP tools. This guide is the whole reference: read it once, then use the loop at the end.

## Manifest

```json
{
  "id": "focus-timer",
  "name": "Focus timer",
  "version": "1.0.0",
  "description": "A timer in a chat's header.",
  "icon": "icon.svg",
  "entry": "init.luau",
  "permissions": ["ui.add", "sessions.read"],
  "ui": { "change": [], "replace": [] },
  "tools": []
}
```

- `id`: 2 to 40 lowercase letters, digits and dashes. `templates`, `zeren` and `app` are taken.
- `icon`: an SVG in your plugin's folder (`"icon.svg"`, 32 KB at most). Zeren draws it tinted in the text's colour on a neutral tile where your plugin is listed (Settings → Plugins, and your plugin's settings page), so draw it in one colour (`currentColor`) on a square viewBox. Without one, your plugin gets its initial on a coloured tile. GitHub's is the GitHub mark.
- `permissions`: only what you use. The person allows them once, in Zeren. Asking for more later asks them again.
- `ui.change`: the selectors you replace, each written exactly as in `zeren.place { at = … }`. Needs `"ui.change"`.
- `ui.replace`: whole screens you replace (`"@app/screens/…"`; never `@app/screens/plugins` or `@app/screens/shell/settings`). Needs `"ui.replace"`. A screen replacement runs as Zeren's own code; prefer `ui.change`.
- `tools`: tools agents can call, each `{ "name", "description", "input" }`. `name` is `^[a-z][a-z0-9_]{0,40}$` without `__`, and `input` is a JSON Schema. Needs `"tools"`.
- `settings`: what the person can set for your plugin (see [Settings](#settings)). Needs no permission.
- `commands`: the programs it runs as you (`["gh", "git"]`), each by its name as it's found on the PATH, never a path. Needs `"commands"`, and the person sees each name when allowing it (see [Programs and a database](#programs-and-a-database)).
- `service`: its headless half (`"service.luau"`), which Zeren's engine runs on each device that allowed the plugin. Needs `"service"` (see [Services](#services)).
- `network`: `{ "hosts": ["api.example.com", "*.example.org", "localhost:8080"] }`, the hosts its requests may reach, from its window and its service alike (see [Reaching the network](#reaching-the-network)). Without it, `network` reaches any public address and never this device, your network or a cloud's metadata address. With it, only those hosts; a private one (`localhost`, `10.…`, `nas.local`) is reachable only when it's named exactly, and the consent sheet says so. `Authorization` and `Cookie` headers go only to hosts named here. Needs `"network"`.

Your own plugins are signed as yours, and re-signed and reloaded each time you save.

### Permissions

| permission | lets the plugin |
|---|---|
| `ui.add` | place nodes before or after Zeren's elements (`zeren.place`), add menu items |
| `ui.change` | replace the elements listed in `ui.change` |
| `ui.replace` | replace the screens listed in `ui.replace` |
| `sessions.read` | read sessions' title, status, project, device, times, branch, worktree |
| `messages.read` | read the open chat's messages (role, text) |
| `profile.read` | read your id, name, username, avatar |
| `account.email` | read your email (unless you hide email addresses) |
| `files.read` | read file paths, and folders: a session's (`cwd`) and a project's (`path`) (without it a file is an id of the plugin's own) |
| `panel.tabs` | open tabs of its own in a chat's panel |
| `pages.add` | add pages of its own to the sidebar |
| `navigate` | open chats and pages (`zeren.open`) |
| `compose` | put text in the message box (`zeren.compose`), or a review comment on it (`zeren.compose_comment`); you still send it |
| `github`, `github.write` | read (and write) GitHub through Zeren's engine (older calls; prefer `commands` with `gh`) |
| `network` | the internet: `ui.fetch`, and remote images (`ui.image { src = "https://…" }`). See [Reaching the network](#reaching-the-network) |
| `clipboard`, `files`, `web`, `windows` | zgx's clipboard, file dialogs, web views, windows |
| `tools` | tools for agents (the manifest's `tools`) |
| `devices` | send data to itself on your other devices |
| `native` | run a program of its own (the manifest's `mcp`) |
| `commands` | run the programs its manifest's `commands` names, as you, on this Mac (`zeren.run`) |
| `service` | run its service (the manifest's `service`) on this device, with no window open, for its windows and agents here and on your other devices |

## Reaching the network

With `network`, `ui.fetch`, remote pictures and the service's `zeren.fetch` all keep to one rule. It's checked for the address you ask, each redirect, and every address a name resolves to, before anything connects.

| Where | Reached |
|---|---|
| A public address | Yes, unless the manifest names hosts, and then only those |
| This device, your network (`localhost`, `127.…`, `::1`, `10.…`, `192.168.…`, `172.16–31.…`, `100.64–127.…`, `fc00::/7`) | Only if the manifest names it exactly (no `*.`) |
| A cloud's metadata address and link-local space (`169.254.…`, `fe80::`), `0.0.0.0`, `::`, multicast | Never, even named |

**Naming a local host.** Write what the URL says, in `network.hosts`:

```json
"permissions": ["network"],
"network": { "hosts": ["localhost:8080", "127.0.0.1:8080", "[::1]:8080", "nas.local"] }
```

- With a port, only that port. Without one (`"localhost"`, `"nas.local"`), any port on that host, which suits a dev server that changes ports. There's no port wildcard or range.
- `localhost` and `127.0.0.1` are different hosts: name the one you use.
- Naming any host narrows the plugin to the hosts you name, public ones included: a plugin that reaches `localhost:8080` and the internet lists its public hosts too (`*.example.org` covers names under it, never a private address).
- A host that reads as local (`localhost`, an address, `.local`) asks the person again when an update adds it.

A refused request fails with an error that names what to add, such as `add "localhost:3000" to "network": { "hosts": [] } in its manifest`. `Host`, `Connection` and the like can't be set, and `Authorization` and `Cookie` go only to hosts named here. A program it runs (`commands`) isn't under this rule: what it can reach is what you can.

## Placing by selector

```lua
local zeren = require("./zeren")("focus-timer")   -- your plugin's id

zeren.place { at = "#session-header", after = function(): Node
	return ui.text { "25:00" }
end }
```

`at` is a selector. `before`, `after` or `replace` is a function that builds a node. It runs as a component of its own, so `ui.use_state` works in it.

- `#id`: one element. `.class`: every element with that class. `.a.b`: both classes.
- `#a >> .b`: a `.b` inside `#a`. `role:button "Send"`: by role and name.
- **One node per match.** `.session-row` matches every row, so your node is built once per row. Each copy reads its own row's context.
- What fails in your node draws nothing there. It's reported once, as your plugin's, and the rest goes on.
- **A replacement sees what's provided above the replaced element, not the element's own `provide`.** Zeren puts each element's context on its parent for this reason (see the table). To place inside an element that provides something, place by a part inside it (`.session-title`, not `.session-row`).

`describe_elements` (MCP) lists every named element in the window running now: its selector, where it is, what it provides, whether it's present, and whether it's **stable**. Stable names are kept across Zeren's updates and by every template. Place by stable names where you can.

| stable selector | where | provides |
|---|---|---|
| `#sidebar` | the sidebar's list | `sessions` |
| `.session-row` | a sidebar row (place inside: `.session-title`) | `session` |
| `#session-header` | a chat's header (title: `#page-title`) | |
| `.message` | a message (`.message-user`, `.message-assistant`) | `message` |
| `#chat-composer` | the message box | |
| `#new-chat-background` | behind a new chat, under Zeren's effect: replace it (`ui.change`) to offer a wallpaper (see below) | `pointer` |
| `.file-row`, `.file-tab` | a file's row in the tree, its tab | `file` |
| `.panel-tab` | a tab of the side panel | `panel` |
| `.md-code` (+ `.lang-<language>`) | a reply's fenced code block | `code` = `{ lang, source }` |
| `.md-math` | a reply's display formula | `tex` |
| `.md-table`, `.md-image`, `.md-quote`, `.md-heading` | a reply's table, picture, quote, heading | |

Everything in the window also has `profile`, `appearance` and `route`. A chat's page and all in it has `session`.

### Slots: places a plugin offers other plugins

A selector reaches Zeren's elements, never another plugin's. A plugin that wants others to add to what it draws offers a **slot**: `zeren.slot(name, props?)` is a column (styled by `props`) holding what each plugin adds there, named under the offering plugin's id. A raw `ui.slot(name)` must be `<your id>.…` too: any other name, or one starting with `zeren`, fails the plugin as it loads, and its row says why. Put a `provide` above it for what they read there: plain data, or a state of it (never a function).

```lua
-- Git's Changes head: its branch, for whoever adds a button beside it.
ui.row { provide = { git_branch = branch }, zeren.slot("branch", { direction = "row", gap = "$space.1" }) }
```

```lua
-- GitHub: Create pull request there ("ui.add").
zeren.place { at = "slot:git.branch", after = function(): Node
	local branch = zeren.live("git_branch")   -- follows it as it changes
	…
end }
```

`before` and `after` both add to a slot, in plugin order; a slot can't be replaced. Zeren offers slots of its own under `zeren.`, a name no plugin can take: `slot:zeren.panel.branch`, the end of the panel home's branch row (`panel_branch`: `{ session, device, cwd, branch }`), where the Git plugin says what the branch adds over its base. Zeren's Git plugin offers `slot:git.branch` (in its Changes head, `git_branch`: `{ session, device, cwd, branch, upstream, remote, remote_url, pushed, ahead, behind, base, base_ahead, head }`), and in History, each giving `git_commit` (`{ sha, subject, author, email, at, parents, refs, remote_url, session }`): `slot:git.commit` (after a row's refs), `slot:git.commit.face` (over the author's face; `git_face` is its size) and `slot:git.commit.head` (an opened commit's head). The GitHub plugin puts its pull request in the first, and in History the author's picture, the pull request a commit came in by, and View on GitHub.

### Menus a plugin offers

Selectors and slots place nodes; a menu's items are data with a function to run, and a function never crosses from one plugin to another. So a plugin offers a menu by name, and other plugins add items to it the way they add to Zeren's own menus:

```lua
-- Git: a History row's menu, its own items then what others added.
ContextMenu { items = function()
	local entries = { …its own… }
	for _, e in zeren.menu_entries("commit", { sha = c.sha, remote_url = url }) do table.insert(entries, e) end
	return entries
end, row }
```

```lua
-- GitHub ("ui.add"): an item in Git's menu; `run` gets the context Git gave.
zeren.menu { kind = "git.commit", id = "view", label = zeren.s("view"), run = function(c) zeren.open_url(…c.sha…) end }
```

The menu's name is the offering plugin's id and a name (`git.commit`), so only Git offers it. Zeren keeps every plugin's items, tells each plugin only those for its own menus, and runs a chosen one in the plugin that added it, with the context the offering plugin gave (plain data). A plugin runs items only in its own menus.

## Context

Inside a build function:

- `zeren.context(name)`: what the place provides, or nil.
- `zeren.need(name)`: the same, or draw nothing here. Settings → Plugins then says why ("Focus timer needs a session, but it's in the title bar").
- `zeren.live(name)`: a function that reads `name` as it changes (a list that moves: `sessions`).
- `zeren.context_of("session", id)`: a session's record by id, read live in a reactive function.

Read at the top of the build function and close over the value: a function run later (a reactive prop) is outside the build.

**What a place provides is read-only.** Don't pass it to a component as its props (`Board(page)` fails: "attempt to modify a readonly table"). Give the component props of its own: `Board { page = page }`.

Records hold only what you were allowed. A field you weren't allowed is absent, never an error.

| name | value |
|---|---|
| `session` | `{ id }` + `title, status, project, device, updated, created, branch, worktree` with `sessions.read` |
| `message` | `{ id }` + `role, text` with `messages.read` |
| `profile` | `{}` + `id, name, username, avatar` with `profile.read`, `email` with `account.email` |
| `file` | `{ id }` + `path, name` with `files.read` (else an id of your plugin's own) |
| `panel` | `{ id, session, open_tab }` |
| `code`, `tex` | plain data from the block |
| `appearance` | `"light"` or `"dark"` |
| `route` | `{ name, session?, page? }` |
| `pointer` | `{ x, y }`: where the pointer is over the page, 0 to 1 (0.5 at rest). Read it with `zeren.live("pointer")` |

### A new chat's background

A plugin can offer a new chat's wallpaper. Replace `#new-chat-background` (list it in your manifest's `ui.change`) and declare a `wallpaper` setting:

```json
"ui": { "change": ["#new-chat-background"] },
"settings": [ { "kind": "wallpaper", "section": "Wallpaper" }, … ]
```

Zeren draws that row on your settings page: "Use as wallpaper", and while it's on, Zeren's effect (dither, ASCII, halftone, scanlines) to draw over your wallpaper. Only the person turns it on, there. While it's on, a new chat opens on what you build at `#new-chat-background` and Settings → Appearance says the wallpaper is from your plugin, with a way to your page; choosing None or Image there takes it back. One plugin's wallpaper at a time: turning yours on replaces another's. Turned off or removed, Zeren goes back to Appearance's choice. Zeren fades what you draw from the top and lays the effect over it, so draw it plain: fill the box (`absolute`, every edge 0) and keep your pictures in your plugin's folder. The row has no value of yours. Zeren's own Parallax scenes plugin (`plugins-builtin/parallax-scenes`) is this whole.

## Doing things

| call | what | needs |
|---|---|---|
| `zeren.open("home" \| { session = id } \| { settings = page })` | open a page | `navigate` |
| `zeren.compose(text)` | text into the message box | `compose` |
| `zeren.compose_comment { file, line, note, to?, side?, snippet?, key?, session? }` | a review comment on a file or a diff, as a chip on a chat's draft (below) | `compose` |
| `zeren.composed()` | in a build: your comments still on a draft, by `key`: `{ session, hovered }` | `compose` |
| `zeren.toast(text)` | a short note at the window's foot | (always) |
| `zeren.need("panel").open_tab { kind, title, icon?, data? }` | a tab of yours in the chat's panel; asked again, the one there is brought forward (found by `data.key` when you give one, else by its title) | `panel.tabs` |
| `zeren.tab(kind, function(tab) return node end)` | how a kind of your tabs draws (`tab` = `{ kind, title, data }`) | `panel.tabs` |
| `zeren.page { id, title, icon?, build = function(page) … end }` | a page of yours, in the sidebar (`title` may be `zeren.t("…")`, following the language) | `pages.add` |
| `zeren.menu { kind = "session" \| "file" \| "<plugin>.<menu>", id, label, run }` | an item in a row's menu (`run` gets `{ id }` or `{ path, session }`), or in a menu another plugin offers (its context) | `ui.add` |
| `zeren.menu_entries(name, context)` | the items others added to a menu of yours (`<id>.<name>`), as ContextMenu entries | |
| `zeren.store(key, default)` | kept between runs (1 MB): `:get()`, `:peek()`, `:set(v)` | (always) |
| `zeren.every(seconds, fn)` | now and then, 30 s at least; returns a stop function | (always) |
| `zeren.window(peek?)` | `{ shown, active }`: whether the window shows (not hidden, minimised or covered) and is in front, read as states (an effect reading them runs again). Leave work no one would see for when it shows | (always) |
| `zeren.db.exec(sql, params)`, `zeren.db.query(sql, params)` | its own SQLite database: promises of `{ changes, last_id }` and of rows | (always) |
| `zeren.run(program, args, { cwd, stdin, timeout, env })` | a program its manifest names, as you: a promise of `{ status, stdout, stderr }` | `commands` |
| `zeren.send(to, data)`, `zeren.on_message(fn)`, `zeren.devices()` | the same plugin on your other devices | `devices` |
| `zeren.github.watch_pr(session_id)` | in a build: a function reading that session's PR `{ number, title, url, state, base, head }` | `github` |
| `zeren.github.pr(url)`, `zeren.github.diff(url)` | promises of a PR's details or diff | `github` |
| `zeren.github.at(session?, project?)` | the same calls, asked on the device that session's or project's checkout is on; also `.line_comment(url, { path, line, side, commit }, body)`, `.reply(url, thread, body)`, `.resolve(url, thread, resolved)`, `.comment(url, body)` | `github`, writes `github.write` |
| `zeren.github.repository(project)`, `zeren.github.list(project, repository, { filter, after, refresh, cached })` | a project's repository (`owner/name`; its folder stays Zeren's) and its open PRs a page at a time | `github` |
| `zeren.notify { title, body?, session? }` | a notification, as Zeren's own: under Settings → Notifications, a few a minute; a click opens `session`. A promise of `"posted"`, `"off"`, `"held"`, `"paced"` or `"unavailable"` | `notifications` |
| `zeren.open_url(url)` | an http(s) link, where you open links (Settings → Links) | `links.open` |
| `zeren.sessions()`, `zeren.projects()` | your sessions and projects by id, as they change | `sessions.read` |

**Cost.** Zeren times each build of your nodes. Past 1 ms a build (a row's badge) or 8 ms of builds in one frame, your row in Settings → Plugins says so ("Slow: 4.2 ms per row"), and `plugin_status` gives the numbers. Do the work outside the build (in `zeren.every`, or once per id), and read what changes in reactive functions.

**Commands and shortcuts.** zgx's own `ui.command { id, title, menu, key, run }` works in a plugin. It shows in Zeren's menu bar as `<plugin>/<id>`, with `key` as its shortcut (`"cmd+shift+t"`). A shortcut Zeren (or an earlier plugin) already has stays theirs: yours shows without it, and your row in Settings → Plugins says so.

A shortcut needs a modifier: `"k"`, `"enter"`, `"f5"` or Shift alone on a character (`"shift+k"`) would take that key from every field, page and terminal. Written so, the plugin fails as it loads, and its row says why. A key alone that a state or function gives is dropped: the command stays in the menu bar without it, and the row says so. For letters while your own view has focus, use `on_key` on your element, or `panel_key` in the panel. A command's `id` can't start with `zeren`, those names being Zeren's own.

**Tools for agents.** List each in the manifest's `tools` (and ask `"tools"`), then:

```lua
zeren.tool { name = "count_words", run = function(args, call)
	local words = 0
	for _ in string.gmatch(args.text or "", "%S+") do
		words += 1
	end
	return { words = words }
end }
```

`run` returns the answer, or `error()` fails the call. It runs as a task, so it can await (`ui.fetch`). `call.chat` is the chat that called. Tools run while Zeren is open on this device; a plugin with a [service](#services) can answer them there instead, with no window open.

### Review comments on the composer

`zeren.compose_comment` leaves a review comment on a file or a diff on a chat's draft (`session`; without one, the draft shown), as the files pane leaves its own: a chip in the composer naming the file and lines (`graph.rs:10–12`), its note under it. Nothing goes until the person sends. Then, after their words, every comment goes in Zeren's review comments block, as production's composer writes it (crates/ui `comments.rs`), so the agent reads each as its file, its lines, its side and its note, with the code under it:

~~~
Comments on the diff (each cites the file and line it belongs to; L = line number in the original file, R = in the changed file):
- src/graph.rs:10-12 (R): Keep the old name.
  ```
   fn lanes() {}
  -fn rows() {}
  +fn rows(n: usize) {}
  ```
~~~

`file` is the project's path; `line` (and a range's last, `to`) the line numbers; `side` `"L"` for the original file's lines (a line taken out), `"R"` for the changed file's; `snippet` the code (at most 40 lines go); `note` what the person said. Without a `side`, it's a comment on a file (`- src/app.rs:12: …`, under "Review comments").

Give each a `key` of your own: `zeren.composed()` lists yours still on a draft by it (taken off or sent, gone), and says which one's chip is under the pointer (`hovered`), so the lines it was left on can keep a mark and light up as its chip is pointed at. Zeren's Git plugin does all of this in its Changes (`plugins-builtin/git/changes.luau`): the diff's + (or a drag from it over lines) opens `ds/comment`'s composer under them, using `ds/git_changes`'s `on_comment`, `ranges` and `marks`.

## Programs and a database

A plugin can do real work of its own: run the command-line tools you already use, and keep what it learns.

**Programs** (`commands`). Name the programs in the manifest and ask for the permission. The person sees each one, and a shell or an interpreter (`sh`, `python3`, `node`) says plainly that it gives you full control of their computer, so name one only if you must:

```json
"permissions": ["commands"],
"commands": ["gh"]
```

```lua
zeren.run("gh", { "api", "user", "--jq", ".login" }):after(function(r)
	if r.status == 0 then zeren.toast(`Signed in to GitHub as {r.stdout}`) end
end)
```

- Each runs as you, on this Mac, with your login shell's PATH and your sign-ins (gh's GitHub login, git's credentials). The consent sheet names the programs and says so. Most programs can run others or write any file, so the sheet says a plugin that names one (git, gh, a shell, an interpreter, or anything not known to be harmless) can do anything you can, and asks every time.
- No shell: `args` reach the program exactly as given, so nothing in them is expanded or split. `stdin` is written to it; `cwd` (an absolute folder) is where it runs.
- `env` sets harmless variables over yours: `LANG`, `LC_…`, `TZ`, `TERM`, `NO_COLOR`, `FORCE_COLOR`, `COLUMNS`, `CI`, git's identity (`GIT_AUTHOR_…`, `GIT_COMMITTER_…`), `GIT_TERMINAL_PROMPT`, `GIT_OPTIONAL_LOCKS`, `GIT_INDEX_FILE` (a `.index` file in a repository's git folder), gh's quiet switches (`GH_PROMPT_DISABLED`, …, `GH_REPO`), and your own `ZEREN_PLUGIN_…` names. Nothing that changes what runs or what loads into it (`PATH`, `GIT_SSH_COMMAND`, `BASH_ENV`, `NODE_OPTIONS`, `PYTHON…`, `DYLD_…`, …).
- git runs as git's own commands only (no aliases, no `git-<x>` from the PATH), with `-c` only for how it prints (`core.quotepath`, `color.…`, `core.pager=cat`, `user.name`, …), never `--upload-pack`, `--exec`, `--output`, `--template` or `--config`, `-C` inside the run's `cwd`, and `git config` read only. Every run has a repository's own fsmonitor turned off, no `ext::` transport, no pager, editor or prompt. Hooks run only where git would run yours and Zeren knows the checkout: a commit, merge, pull, push, rebase or branch switch from a service, in one of this device's chats' or projects' folders. Reading, anything elsewhere, and runs from a window run none. gh runs its own commands only: never `alias`, `extension`, `codespace`, `auth` except `auth status` (without the token), or `config` except reading.
- A run waits `timeout` seconds at most (60 unless you say, 300 at most), then is stopped (`status` -1). What it prints is kept up to 16 MB.
- Only the programs you named run; anything else is refused.

**A database** (`zeren.db`, no permission). Your plugin's own SQLite file: removed with the plugin, 100 MB at most, and no other file reachable through it. Use it for a cache, an index or a history, anything past what `zeren.store`'s small JSON file holds.

```lua
zeren.db.exec("CREATE TABLE IF NOT EXISTS seen (url TEXT PRIMARY KEY, at INTEGER)")
zeren.db.exec("INSERT OR REPLACE INTO seen VALUES (?, ?)", { url, os.time() })
zeren.db.query("SELECT at FROM seen WHERE url = ?", { url }):after(function(rows)
	local first = rows[1] and rows[1].at
end)
```

Parameters are always bound (`?`), never put into the SQL. `exec` with no parameters runs a script of several statements (a schema). Each call is a promise.

Zeren's GitHub plugin is built this way: it asks GitHub's own API through `gh` (GraphQL and REST), keeps what it read in its database so a pull request opens at once, and polls each repository once a minute for the sidebar's badges.

## Services

A service is your plugin's headless half: `service.luau`, run by Zeren's engine on each device, with no window open. Your windows call it on this device or on another of yours, and agents' tools can be answered by it. It's how a plugin does real work where the work is: a Git plugin runs `git` on the device a chat's checkout is on, whether that's this Mac or a devbox.

```json
"service": "service.luau",
"permissions": ["service", "commands"],
"commands": ["git"]
```

```lua
-- service.luau
local zeren = require("./zeren")("my-git")

zeren.serve("status", function(args, call)
	local r = zeren.run("git", { "-C", args.cwd, "status", "--porcelain" }):await()
	if r.status ~= 0 then error(r.stderr, 0) end
	zeren.emit("checked", { cwd = args.cwd })
	return { clean = r.stdout == "" }
end, { safe = true })
```

```lua
-- init.luau: on the device the session's checkout is on (nil: this one)
zeren.service(session.device):call("status", { cwd = session.cwd }):after(function(s)
	zeren.toast(if s.clean then "Clean" else "Changes")
end, function(err)
	zeren.toast(zeren.reason(err))       -- "offline", "not_approved", …
end)
```

**Each device decides for itself.** A device runs your service only if the plugin is installed there, on, and allowed (with `service`), in the same version as the caller's, signed by the same author. Its `commands` run as the person on that device, with that device's sign-ins. The person allows it once per device, as with every plugin.

**In service.luau**, `require("./zeren")(id)` gives the service's helpers (the window's `ui.*` isn't there):

| call | what | needs |
|---|---|---|
| `zeren.serve(method, fn, { safe? })` | answer `zeren.service(…):call(method, args)`: `fn(args, call)` returns the answer, `error()` fails it. `call.from` is the calling device (`{ id, name }`). `safe = true`: running it twice does no harm (see below) | |
| `zeren.tool { name, run }` | answer an agent's tool (one the manifest's `tools` lists), with no window open | `tools` |
| `zeren.every(seconds, fn)` | now and then (30 s at least), even while nothing calls it: Zeren starts the service to run it. Register timers at the top level, in the same order each time | |
| `zeren.on_stop(fn)` | told before it stops (idle, an update, Zeren quitting) | |
| `zeren.emit(event, data)` | to your windows: those on this device, and those watching from another (`:on`). 64 KB at most | |
| `zeren.watch(folders, fn, { quiet? })` | changes under whole paths on this device, each all the way down: `fn(changed)` once a burst has been quiet `quiet` ms (250), `changed` by folder: its changed paths (relative, `/`-separated), or `true` when anything may have (as it starts, too many, events lost). Reads aren't changes. Returns a stop function; it lasts while the service runs. Pair it with `emit` to tell windows, instead of their polling | `files.read` |
| `zeren.run(program, args, { cwd, stdin, timeout, env })` | a promise of `{ status, stdout, stderr }`, as the window's | `commands` |
| `zeren.fetch(url, { method, headers, body, timeout })` | a promise of `{ status, headers, body }` (http and https; public addresses, or the manifest's `network.hosts`) | `network` |
| `zeren.folders()` | a promise of this device's chats' and projects' folders (whole paths) | `sessions.read` |
| `zeren.notify { title, body?, session? }` | as the window's | `notifications` |
| `zeren.store(key, default)` | `:get()`, `:set(v)`: kept on this device between runs (1 MB), the service's own (not the window's store) | |
| `zeren.sleep(seconds)`, `zeren.task(fn)` | a promise done later; run `fn` as a task of its own | |
| `zeren.json.encode(v)`, `zeren.json.decode(text)`, `zeren.device` | JSON; this device's `{ id, name }` | |

Handlers, tools, timers and `:after` callbacks each run as a task, so `:await()` works in them. A service shares nothing with your windows but what it answers and emits.

**In your windows:**

- `zeren.service(device?)`: the service on `device` (an id: a session's or project's `device`), or on this device without one.
- `:call(method, args, { timeout })`: a promise of what the method returned. 30 seconds unless you say, 300 at most.
- `:on(event, fn)`: `fn(data, event)` for each `zeren.emit` it makes. For another device, its events reach this one while a window here is open. Returns a function that stops listening.
- `zeren.reason(err)`: why a call failed, as a word.

**Failed calls are yours to handle.** Zeren never retries a call for you, and says why it failed:

| reason | the device that was called… |
|---|---|
| `offline`, `timeout` | couldn't be reached, or didn't answer in time |
| `not_installed`, `off`, `not_approved` | doesn't have the plugin, has it turned off, or hasn't allowed it (or its service) yet |
| `version_mismatch`, `other_author` | has another version of it, or someone else's plugin by the same id |
| `no_service`, `not_loaded` | has a version with no service, or one whose service.luau failed to load (the message says why) |
| `unknown_method`, `failed` | doesn't serve that method, or your handler failed (`error()`, or a refused program); the message is yours |
| `interrupted` | stopped during the call, and the method isn't `safe` |
| `old_engine` | runs a Zeren from before services |

The one exception: when the service stops during a call (it ran out of memory, or crashed), a method marked `safe = true` runs once more on a fresh start. Mark only what can run twice: reading, not posting.

**When it runs.** A service starts on its first call, tool call or timer, and stops after 5 minutes with nothing to do. An update loads the new version beside the old one, which answers until the new one has loaded and finishes the calls it has. Each call or resumed task has 2 seconds of work before it fails (await between parts: waiting costs nothing), and the service has 64 MB. `plugin_status` shows the service on this device: running or not, what it serves, its last log lines (`print`) and why it didn't load.

Zeren's Git plugin (`plugins-builtin/git`) is built this way: its service does every piece of git work on the device a chat's checkout is on (status, what changed through an index of its own via `GIT_INDEX_FILE`, diffs, staging, commits, pushes, branches, history), and its window says each refusal in its own words. `plugins-dev/checkout-status` is the small version, whole: a session row's menu asks the service on that session's device for the checkout's branch and changes, and agents get the same through a tool.

## Settings

Declare settings in the manifest; Zeren draws their page. While your plugin is on, it has a page of its own in Settings, under "Plugins", titled with its name and built from Zeren's own rows, so it looks native in every theme and template. You never draw it, and no plugin can draw into it. Settings search finds each one.

```json
"settings": [
  { "id": "show", "kind": "switch", "label": "Show the timer", "description": "In each chat's header.", "default": true },
  { "id": "length", "kind": "number", "label": "Length", "min": 5, "max": 90, "unit": "minutes", "default": 25 },
  { "id": "sound", "kind": "choice", "style": "segmented", "label": "Sound",
    "options": [ { "value": "bell", "label": "Bell" }, { "value": "none", "label": "None" } ], "default": "bell" }
]
```

Each setting has an `id` (letters, digits, `- _ .`), a `kind`, a `label`, and optionally a `description`, a `default` and a `section` (consecutive settings with the same `section` share a titled card). By kind:

| kind | value | extra fields |
|---|---|---|
| `switch` | `true` / `false` (default off) | |
| `choice` | one option's `value` (default the first) | `options` `[{ value, label, description? }]` (an option's `description` stands in for the setting's while it's chosen), `style` `"select"` or `"segmented"` |
| `text` | text, 2000 characters at most | `placeholder` |
| `number` | a number, kept within `min` and `max` | `min`, `max`, `step`, `unit` |
| `shortcut` | `"ctrl+alt+space"` (Control, Option or Command held), or `""` | |
| `folder` | the absolute path of a folder the person chose, or `""` | |
| `wallpaper` | none of yours: Zeren's "Use as wallpaper" row, for a plugin that replaces `#new-chat-background` ([above](#a-new-chats-background)) | |

Read them in your code (no permission):

```lua
local length = zeren.setting("length")        -- :get() follows it, :peek() doesn't
local all = zeren.settings()                  -- every value by id
zeren.on_setting(function(id, value, was)     -- each change the person makes
	zeren.toast(`{id} is now {tostring(value)}`)
end)
```

**Status.** Your plugin can say how a setting stands, and Zeren draws it beside the control: a short badge in a fixed tone (`neutral`, `success`, `warning`, `danger`) and words in the description's place. Plain text only; it goes when your plugin is off.

```lua
zeren.set_status("shortcut", { badge = "Set up", tone = "warning", text = "Another app has these keys. Choose others." })
zeren.set_status("shortcut", nil)   -- cleared
```

**Permission rows** (`{ "kind": "permission", "permission": "screen-recording" | "accessibility" }`) are for Zeren's own plugins only; anyone else's is left out. Zeren supplies the row's words, whether it's granted, the button to grant it and Check again.

- Only the person sets them, on that page; your plugin reads them.
- A setting never set reads its `default`, so changing a default in an update reaches everyone who left it alone. A value that no longer fits (an option you dropped) reads the default again.
- What the person sets is kept for your plugin alone, across restarts, and removed with the plugin.

## Translations

Zeren speaks the language chosen in Settings → General → Language (English or 简体中文 today, or the Mac's own). A plugin can too: its words go in `locales/`, beside `zeren-plugin.json`, a file per language, with `en.json` the source:

```
my-plugin/
  zeren-plugin.json
  init.luau
  locales/
    en.json        { "timer": { "start": "Start", "left": "{n, plural, one {# minute} other {# minutes}} left" } }
    zh-Hans.json   { "timer": { "start": "开始", "left": "还剩 {n} 分钟" } }
```

```lua
ui.button { zeren.t("timer.start"), on_click = start }   -- follows the language as it changes
ui.text { zeren.t("timer.left", { n = minutes }) }       -- a state or function argument follows too
tooltip = zeren.s("timer.start")                         -- the text now, where only a string goes
zeren.toast(zeren.s("timer.done"))
```

- **Keys** are nested in the file and dotted in code (`timer.start`). Messages are ICU MessageFormat: `{name}`, `{n, plural, one {# item} other {# items}}` (and `=0 {…}`), `{x, select, a {…} other {…}}`. Plurals follow English's rules (`one` for 1, else `other`); a language with one form (Chinese) writes `other` alone.
- **`zeren.t(key, args)`** is for text (a child, `ui.text`, a label): it follows the language, and the arguments if they're states or functions. **`zeren.s(key, args)`** is the text now, for what takes only a string (a tooltip, a toast, a menu item's label). Read in a function, it follows as well.
- **Falling back:** a language the plugin has no file for, or a key that file lacks, shows the plugin's English (`zh-Hans`, then `zh`, then `en`), and a key English lacks shows the key itself. A plugin without `locales/` loses nothing: plain strings work as they always have.
- **Zeren's own words** in what the plugin uses (the design system's buttons, menus and states) follow the language by themselves.
- Language files are part of the plugin: signed and shipped with it like any other file, up to 1 MB together. Zeren reads them as the plugin loads, so after editing one, save `init.luau` (or turn the plugin off and on) to see it.
- `ui.t` is Zeren's own catalog, not the plugin's: use `zeren.t` for the plugin's words.

`plugins-dev/session-words` is a working example: session-status's word in a chat's header, in English and Chinese.

## The loop

1. **`create_plugin { id, name, description, permissions }`** makes the folder and signs it as yours. Its reply gives the folder, its files and a link to this guide.
2. **Edit `init.luau`** (and the manifest, for permissions). Zeren re-signs and reloads it on every save. If the manifest asks for something new, the person allows it in Zeren.
3. **`plugin_status { id, snapshot = true }`** answers without a person:
   - its status (on, off, pending review, tampered, broken, and why) and what it was allowed;
   - each place it made: its selector, how many elements it matched and their boxes;
   - what its Settings row says (a need its place doesn't give, places this template lacks, an error);
   - with `snapshot`, a PNG of the window around its nodes. Read the file at `snapshot`.
4. **Fix and save**, then ask again. Matched 0? Check the selector against `describe_elements`. A need? Place it inside something that provides it.

`list_plugins` lists them all. Selectors that aren't stable may move; templates may rename elements. `plugin_status` and Settings show "Not shown in this template" when yours aren't there.

## Sharing and installing

A plugin travels as its folder, signed. The person installs it with Settings → Plugins → Install…; an agent uses these tools:

- **`install_plugin { path }`** installs a `.zeren-plugin` file. **`install_plugin { url, ref?, folder? }`** installs from a link:
  - a link to a `.zeren-plugin` file;
  - a GitHub release (`…/releases/latest`, `…/releases/tag/<tag>`), from its `.zeren-plugin` asset;
  - a git repository: `owner/repo`, its GitHub page, `…/tree/<branch>/<folder>`, or any https git URL.
- **`update_plugin { id }`** installs the newest version from wherever the plugin came from. **`update_plugin { check = true }`** only reports what's new: each update's version and the permissions it adds or removes.

Zeren checks the signature over every file before anything is written, so an unsigned or changed plugin is refused.
- **Signed by the person's own key:** it turns on at once.
- **Signed by anyone else:** it waits until the person allows it in Zeren. The reply says `waiting`; you can't allow it for them.
- **An update:** it must be signed by the same key as the installed version. One that asks for a new permission still installs, but waits for the person to allow just that.
- **Removing a plugin** (Settings) takes away its folder, what it stored, its pages and its placements.

To publish a plugin, commit its folder with its signed `zeren-plugin.json` to a repository, or attach the `.zeren-plugin` file to a GitHub release. Zeren re-signs your own plugins as you save them, so what you commit is already signed. The package format (`zeren-plugin:v1`) is the same signed package templates use: the manifest, plus every file as base64.

## Three examples

### A badge on each sidebar row

```json
{ "id": "row-branch", "name": "Row branch", "version": "1.0.0", "entry": "init.luau",
  "permissions": ["ui.add", "sessions.read"] }
```

```lua
local zeren = require("./zeren")("row-branch")

zeren.place {
	at = ".session-title",          -- inside each row, so the row's session is above it
	after = function(): Node
		local id = zeren.need("session").id
		return ui.text {
			function(): string
				return zeren.context_of("session", id).branch or ""
			end,
			text_size = "$font.size.xs",
			color = "$text.tertiary",
		}
	end,
}
```

### A widget in a chat's header

```json
{ "id": "focus-timer", "name": "Focus timer", "version": "1.0.0", "entry": "init.luau",
  "permissions": ["ui.add"] }
```

```lua
local zeren = require("./zeren")("focus-timer")

zeren.place {
	at = "#page-title",              -- in #session-header, which has the session
	after = function(): Node
		zeren.need("session")        -- only on a chat's page
		local left = ui.use_state(25 * 60)
		-- (A timer belongs to the node it's made in: it stops with it.)
		ui.every(1000, function()
			left:set(math.max(0, left:peek() - 1))
		end)
		return ui.text {
			function(): string
				local s = left:get()
				return string.format("%d:%02d", s // 60, s % 60)
			end,
			tabular = true,
			text_size = "$font.size.sm",
			color = "$text.secondary",
		}
	end,
}
```

### A renderer for a kind of code block

Replies' fenced blocks are `.md-code.lang-<language>`, and each provides `code`. Replace one kind, and the rest stay code blocks. A block still streaming in is built again as it grows. With your plugin off, it's a code block again. (`plugins-dev/md-hello` is this, with tests.)

```json
{ "id": "mermaid-view", "name": "Mermaid diagrams", "version": "1.0.0", "entry": "init.luau",
  "permissions": ["ui.change"], "ui": { "change": [".md-code.lang-mermaid"] } }
```

```lua
local zeren = require("./zeren")("mermaid-view")

zeren.place {
	at = ".md-code.lang-mermaid",
	replace = function(): Node
		local code = zeren.need("code")      -- { lang = "mermaid", source = "graph TD; A-->B" }
		return ui.canvas {
			w_full = true,
			h = 240,
			draw = function(c)
				-- lay code.source out and draw it: boxes, arrows, labels
			end,
		}
	end,
}
```

## Zeren's components

Zeren lends your plugin its design system: the same components Zeren's own screens are built from, from whichever template is active, so your plugin looks native in every theme and template. It needs no permission. Require the modules you use from `@ds/…` (not `@app/…`, which plugins can't reach):

```lua
local Button = require("@ds/button")
local Badge = require("@ds/badge")
local Settings = require("@ds/settings")
local git = require("@ds/git")             -- a change's pieces: header, diff, checks, activity

zeren.tab("my-review", function(tab: any): Node
	return ui.column {
		w_full = true,
		gap = "$space.5",
		git.header({ state = "open", number = 12, title = tab.title, branch = "fix", base = "main", added = 3, removed = 1 }),
		Settings.Group {
			Settings.Row { label = "Checks", description = "2 of 3 passed", control = Button { "Re-run", tone = "secondary" } },
		},
	}
end)
```

- **Use them before drawing your own.** Text (`@ds/text`), buttons and icon buttons, fields, selects, tabs, badges, callouts, empty states, settings cards and rows, markdown, comments and threads (`@ds/comment`, `@ds/inline_block`), and the git pieces (`@ds/git`, `@ds/git_changes`). They hold the theme's spacing, corners, focus rings and motion, which is what makes a plugin look like part of Zeren rather than beside it. `ui/ds/*.luau` is their source, each with its usage at the top, and the gallery (`zeren-zgx gallery`) shows each one.
- **In your plugin's own VM.** The modules load in your sandbox, with state of their own; nothing is shared with Zeren's copy or another plugin's. They lend no authority: what they do is limited by your permissions, as your own code is. Set a markdown link's destination with `require("@ds/links").opener = function(url) zeren.open_url(url) return true end` (`links.open`).
- **Require a module, not `@ds` itself.** `@ds` loads every component Zeren has; `@ds/button` loads one.
- **A tab or a page fills its room.** What `zeren.tab` and `zeren.page` build is given the room the panel or page has: `h_full` on it, then scroll inside (`ui.scroll`) or keep to the height and scroll in parts.

- **Faces take a picture from anywhere you can reach.** A person (`{ id, name, src }`) shows `src` over their gradient, which shows until it arrives or if it never does; `src` may be a remote address with `network`.
- **The lightbox works in a plugin.** Put one `require("@ds/lightbox").Host()` in what you build (a tab's, a page's); a markdown picture then opens whole over the window on a click, as a chat's does. (Zeren tells your plugin the window's size for it, as `ui.shared("zeren.window")`, since `ui.window` isn't a plugin's.)

Zeren's GitHub plugin (`plugins-builtin/github`) is built this way throughout, with nothing of Zeren's made for it: its pull request viewer and board are the gallery's review design on `@ds/git`, its sidebar badge is `@ds/badge`'s, people's faces are their GitHub pictures (loaded with `network`), and everything it knows comes from GitHub through `gh` (`commands`), kept in its database. Its words are in `locales/en.json` (`zeren.t`, `zeren.s`), so it follows the language like the rest of Zeren.

## Style

Prefer Zeren's components (above). Where you draw your own, use Zeren's tokens, never raw values: colours (`$text.primary`, `$text.secondary`, `$accent`, `$border`), spacing (`$space.1`…`$space.8`), radii (`$radius.sm`…`$radius.xl`) and type (`$font.size.xs`…`$font.size.lg`). Your nodes then follow the person's theme and accent. Inside something rounded, a corner is the outer radius less the inset.
