# Zeren

Control your coding agents (Claude Code, Codex, Cursor, Devin, Grok, Hermes, Pi, Antigravity) locally by default, with optional multi-device sync.

*English | [简体中文](README.zh-CN.md) | [한국어](README.ko.md) | [日本語](README.ja.md)*

![Zeren desktop app](docs/media/readme/app-screenshot.jpg)

## Desktop app

Download the latest release for your platform from [GitHub Releases](https://github.com/femboypuppy/zeren/releases/latest):

- **macOS** — `zeren-<version>-macos-arm64.dmg`
- **Windows** — `zeren-<version>-windows-x86_64-setup.exe`
- **Linux** — `zeren-<version>-linux-<arch>.tar.gz`, then run its `install.sh`

No account or network connection is needed; sessions stay on your device. The app updates itself.
When upgrading from Zeron, install Zeren once from the release page. Zeren moves your existing chats and settings to its new data directory on first launch.

## Headless (CLI)

For servers and other machines without a display, such as a VPS that keeps agents running after you close your laptop. Linux only:

```bash
tar -xzf zeren-*.tar.gz
./zeren-*/install.sh
zeren status
```

The installer starts the engine as a background service that survives reboots.

```bash
zeren status      # local/synced mode and engine status
zeren update      # update to the latest release
zeren daemon start|stop|restart|status
```

## Multi-device sync (optional)

Sign in to start an agent on one device and follow or drive it from another:

```bash
zeren daemon stop
zeren login        # or: zeren logout to return to local-only
zeren daemon start
```

Devices signed in to the same account can read and write each other's workspace files, so only sign in devices you trust. Existing local sessions are never uploaded.

## Sponsors

Thank you to [The Context Company](https://www.thecontextcompany.com/) for sponsoring Zeren. You can help fund Zeren's development too by [becoming a sponsor on GitHub](https://github.com/sponsors/zeronsh).

---

Developing or curious how it works? [Ask DeepWiki](https://deepwiki.com/zeronsh/zeron) or check out [ARCHITECTURE.md](ARCHITECTURE.md).

Licensed under the [MIT License](LICENSE).
