# Magical Diary（魔法日记）

一个适合在鸿蒙 / Android 平板上演示的《哈利·波特》风格互动日记。

用手写笔在空白纸面上写字，停笔片刻后，墨迹会被日记“吸收”。日记读取你的文字，以 Tom Riddle 的口吻思考，并用手写体逐笔写出回答。最近的对话会保存在平板本地，因此它还能记得之前写过的内容。

> 本项目是非官方同人小作品，仅供学习与娱乐，与 J. K. Rowling、Warner Bros.、reMarkable AS 或 Moonshot AI 无关。

## 效果与功能

- 手写笔输入，浏览器支持时会读取压感
- 停笔约 2.8 秒后自动提交
- 原始墨迹逐渐模糊、消失
- Kimi 视觉模型直接识别整张手写页面
- Tom Riddle 风格人格回复
- Dancing Script 手写体逐笔出现
- 最近对话保存在平板本地
- 第一次打开时填写 API Key，之后自动读取
- 不需要 root，不需要制作 APK

## 运行条件

- 能运行 Termux 的鸿蒙 / Android 平板
- 可用的网络连接
- Kimi 开放平台 API Key
- 建议使用支持手写笔 `PointerEvent` 的现代浏览器

ChatGPT Plus、Kimi 网页会员等订阅通常不等同于 API 额度。本项目使用的是 Kimi 开放平台 API Key。

## 一次性安装

打开 Termux，把下面整段复制进去执行：

```bash
pkg update -y
pkg install -y git rust
cd "$HOME"
git clone https://github.com/KingDragon-yc/Magical-Diary.git
cd Magical-Diary
cargo build --release --bin riddle-web
termux-wake-lock
nohup ./target/release/riddle-web > riddle.log 2>&1 &
sleep 2
termux-open-url http://127.0.0.1:8787
```

第一次编译需要下载 Rust 依赖，可能要等待几分钟。

浏览器打开后：

1. 填入 Kimi API Key。
2. 点击 **Bind the secret**。
3. 在纸面上用手写笔写字。
4. 停笔约 2.8 秒，等待日记回应。

API Key 会保存在：

```text
$HOME/.config/riddle/oracle.env
```

文件权限会设置为仅 Termux 当前用户可读写。Key 不会保存在网页、本仓库或浏览器 Local Storage 中。

## 以后启动

安装和编译完成后，再次使用只需执行：

```bash
cd "$HOME/Magical-Diary"
termux-wake-lock
nohup ./target/release/riddle-web > riddle.log 2>&1 &
sleep 1
termux-open-url http://127.0.0.1:8787
```

右上角有一个很淡的菱形：

- 轻点：进入或退出全屏
- 长按约 1.2 秒：重新填写 API Key

## 更新

仓库有新版本时执行：

```bash
cd "$HOME/Magical-Diary"
git pull
cargo build --release --bin riddle-web
pkill riddle-web 2>/dev/null
nohup ./target/release/riddle-web > riddle.log 2>&1 &
sleep 1
termux-open-url http://127.0.0.1:8787
```

## 停止

```bash
pkill riddle-web
termux-wake-unlock
```

## 排查问题

### 浏览器无法打开页面

查看服务日志：

```bash
cat "$HOME/Magical-Diary/riddle.log"
```

确认程序是否仍在运行：

```bash
pgrep -a riddle-web
```

### 写字后没有回应

- 检查平板是否联网
- 长按右上角菱形，重新填写 API Key
- 查看 `riddle.log` 中的 API 错误
- 确认 Kimi 开放平台账户仍有可用额度

### 手写笔没有压感

压感取决于平板系统和浏览器是否向网页开放 `PointerEvent.pressure`。没有压感时仍然可以正常书写，只是笔画粗细变化较少。

### Termux 被切到后台后服务停止

请允许 Termux 在后台运行，并关闭系统对 Termux 的自动电池优化。启动命令中的 `termux-wake-lock` 能减少休眠中断，但部分鸿蒙设备仍需手动允许后台活动。

## 手动配置（可选）

如果不想在首次启动页面填写，也可以在项目目录创建 `oracle.env`：

```env
RIDDLE_OPENAI_KEY=你的_Kimi_API_Key
RIDDLE_OPENAI_BASE=https://api.moonshot.cn/v1
RIDDLE_OPENAI_MODEL=kimi-k2.6
RIDDLE_OPENAI_MAX_TOKENS=800
RIDDLE_OPENAI_THINKING=disabled
RIDDLE_TZ_OFFSET=8
```

本地记忆默认位于：

```text
$HOME/.local/share/riddle/memories
```

删除该目录即可让日记忘记历史。设置 `RIDDLE_MEMORY=off` 可以完全关闭记忆。

## 是否需要部署到服务器？

单台平板演示时不需要服务器。程序和网页都在 Termux 本机运行，浏览器访问：

```text
http://127.0.0.1:8787
```

这种方式延迟低，API Key 也不会发送给浏览器。如果以后部署到公网，需要额外配置 HTTPS、访问认证、请求限流和服务端密钥保护。

## 项目来源

本项目移植自 Maxime Rivest 的开源项目：

- 原项目：[MaximeRivest/riddle](https://github.com/MaximeRivest/riddle)
- 原作者：Maxime Rivest 及原项目贡献者
- 原始创意与实现包括：reMarkable 版本、人格与记忆逻辑、墨迹效果、手写回复合成等

本仓库在原项目基础上增加了面向鸿蒙 / Android 平板的 Termux 本地网页版本，包括浏览器手写画布、本地 HTTP 服务、Kimi 配置和首次启动设置界面。

原项目采用 MIT License，本仓库继续保留并遵循该许可证，详见 [LICENSE](LICENSE)。Dancing Script 字体许可见 [fonts/OFL.txt](fonts/OFL.txt)。
