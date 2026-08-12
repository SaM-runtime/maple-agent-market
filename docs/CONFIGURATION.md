# 設定

為了讓既有使用者平順升級，設定目錄仍使用相容路徑：

- Windows：`%USERPROFILE%\.config\pixtuoid\config.toml`
- macOS / Linux：`~/.config/pixtuoid/config.toml`
- 設定了絕對路徑 `XDG_CONFIG_HOME` 時：`$XDG_CONFIG_HOME/pixtuoid/config.toml`

現行產品只讀四類設定：頂層 `pack-dir`、`[sources]`、`[floating]` 與 `[audio]`。舊辦公室的 `theme`、`max-desks`、`pets`、`last-seen-version` 等 key 會被忽略，不會重新啟用已移除功能。

```toml
pack-dir = "C:/team/authorized-pack"

[sources]
codex = true
claude-code = false

[floating]
width = 720
height = 480
x = 120
y = 80
opacity = 1.0

[audio]
muted = true
volume = 0.35
bgm-path = "C:/Music/your-licensed-track.mp3"
```

## 行為

- 視窗預設為 `360 x 240`，最小 `240 x 160`。
- `opacity` 會限制在 `0.2..1.0`。
- 音訊預設靜音；`volume` 會限制在 `0.0..1.0`。
- `bgm-path` 只讀本機 MP3、WAV、OGG 或 FLAC，不會下載遠端內容。
- CLI 的 `--pack-dir` 優先於設定檔。
- malformed config 只會回到安全預設並發出警告，不會覆寫原檔。

視窗位置、大小、靜音和音量會在操作後寫回設定。`sources`、`connect` 與 `disconnect` 會維持既有註解與不認識的 sibling key。

## CLI 與環境變數

```powershell
.\target\debug\maple-agent-market.exe sources
.\target\debug\maple-agent-market.exe doctor
.\target\debug\maple-agent-market.exe floating --pack-dir C:\team\authorized-pack
```

進階顯示調整保留上游相容環境變數：

- `PIXTUOID_LABEL_SCALE`：字卡比例，限制在 `0.75..2.0`。
- `PIXTUOID_FLOATING_SCALE`：整數 pixel scale，限制在 `1..8`。
- `PIXTUOID_CJK_FONT`：指定本機 CJK 字型檔。
- `PIXTUOID_LOG`：指定 log 檔位置。
- `PIXTUOID_HOOK`：指定 `pixtuoid-hook` 絕對路徑。

這些名稱是 wire / config 相容層，不代表舊 Pixtuoid UI 仍存在。
