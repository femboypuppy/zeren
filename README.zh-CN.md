# Zeren

在本地管理你的编码 agent（Claude Code、Codex、Cursor、Devin、Grok、Hermes、Pi、Antigravity），也可以打开多设备同步。

*[English](README.md) | 简体中文 | [한국어](README.ko.md) | [日本語](README.ja.md)*

![Zeren 桌面应用](docs/media/readme/app-screenshot.jpg)

## 桌面应用

从 [GitHub Releases](https://github.com/femboypuppy/zeren/releases/latest) 下载对应平台的最新版本：

- **macOS** — `zeren-<version>-macos-arm64.dmg`
- **Windows** — `zeren-<version>-windows-x86_64-setup.exe`
- **Linux** — `zeren-<version>-linux-<arch>.tar.gz`，解压后运行里面的 `install.sh`

不用账号，也不用联网，会话就存在这台设备上。应用会自动更新。

## 无界面运行（CLI）

适用于服务器等没有显示器的机器，比如在你合上笔记本之后继续跑 agent 的 VPS。仅支持 Linux：

```bash
tar -xzf zeren-*.tar.gz
./zeren-*/install.sh
zeren status
```

安装脚本会把引擎作为后台服务拉起来，重启之后也会自己回来。

```bash
zeren status      # 查看本地/同步模式和引擎状态
zeren update      # 更新到最新版本
zeren daemon start|stop|restart|status
```

## 多设备同步（可选）

登录后，可以在一台设备上起 agent，换另一台设备接着看、接着操作：

```bash
zeren daemon stop
zeren login        # 或者 zeren logout 切回纯本地模式
zeren daemon start
```

登录同一账号的设备可以读写彼此工作区里的文件，所以只登录你信任的设备。已有的本地会话不会被上传。

## 赞助

感谢 [The Context Company](https://www.thecontextcompany.com/) 对 Zeren 的赞助。你也可以[通过 GitHub 成为赞助者](https://github.com/sponsors/zeronsh)，资助 Zeren 的开发。

---

想参与开发，或者好奇它怎么跑起来的？[Ask DeepWiki](https://deepwiki.com/zeronsh/zeron)，也可以看 [ARCHITECTURE.md](ARCHITECTURE.md)。

采用 [MIT License](LICENSE)。
