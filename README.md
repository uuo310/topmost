# topmost

Windows 窗口置顶小工具。按 `Ctrl+Shift+A` 把当前窗口钉在最前面，该窗口标题栏会冒出一枚青色图钉；点图钉取消置顶。

**直接下载**：<https://github.com/uuo310/topmost/releases/latest/download/TopMost.exe> —— 编好的 exe，单文件绿色，不用装 .NET 或 VC++ 运行库。

## 用法

1. 运行 `TopMost.exe`。程序常驻托盘，不占任务栏。
2. 选中要置顶的窗口，按 `Ctrl+Shift+A`。
3. 该窗口标题栏、最小化按钮左侧出现图钉；点它取消置顶。
4. 托盘图标右键：「开机自动启动」「退出」。

## 说明

- 纯 Win32 API（`windows` crate），没有 UI 框架、没有第三方运行时。exe 约 610 KB，常驻内存约 13 MB，空闲时 CPU 占用 0。
- 64 位（x64）程序，32 位 Windows 跑不了——现在还在用 32 位 Windows 的机器基本见不到了。
- 运行库是静态链接的：`VCRUNTIME140.dll`、`ucrtbase.dll` 这些都不需要，拷到任何一台 64 位 Windows 10/11 上双击就能跑。
- 热键为什么是 `Ctrl+Shift+A`：`Win+Ctrl+T` 是 PowerToys「始终置顶」的默认键；`Win+Shift+字母` 里 A C M P R S T V W 已被系统占用，`Win+Alt+字母` 被 Xbox Game Bar 占用；`Ctrl+Alt+字母` 扫起来虽然全空，实际照样被老软件/显卡面板抢。`Ctrl+Shift+字母` 整片空闲。
- 图钉不是图片，是运行时现画的（4×4 超采样 + 解析式几何），放大缩小都不会糊。程序图标 `app.ico` 在编译期内嵌进 exe，托盘用的是同一份资源。
- 标题栏按钮位置优先读 DWM 的 `DWMWA_CAPTION_BUTTON_BOUNDS`；Chrome/Electron、UWP、Qt 这类自绘标题栏读不到有效值，退回按系统按钮尺寸估算，所以图钉照样落在最小化按钮左边。
- 开机自启写的是当前用户的 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，不需要管理员权限、不往系统目录写东西。程序挪了位置也不要紧：下次启动会自己把登记改到新路径（前提是它本来就开着；没开过不会擅自加）。
- 已适配高 DPI（per-monitor v2），坐标按物理像素计算。

## 编译

需要 Rust 工具链。exe 的图标资源由 Windows SDK 的 `rc.exe` 编译进 PE；找不到 `rc.exe` 时只会警告并跳过这一步（exe 没有文件图标，功能不受影响）。

```
cargo build --release
```

产物是 `target\release\topmost.exe`。仓库里的 `.cargo\config.toml` 打开了 `+crt-static`（静态链接 CRT），所以编出来的 exe 不依赖 VC++ 运行库；删掉那个文件就会回到动态链接、体积小 100 KB 左右，但目标机器上得装过 VC++ 2015-2022 Redistributable。

## 许可

[MIT](LICENSE)。随便用，改、分发、商用都行，保留版权声明即可。
