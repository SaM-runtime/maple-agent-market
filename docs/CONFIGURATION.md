# 設定

為了讓既有使用者平順升級，設定目錄仍使用相容路徑：

- Windows：`%USERPROFILE%\.config\pixtuoid\config.toml`
- macOS / Linux：`~/.config/pixtuoid/config.toml`
- 設定了絕對路徑 `XDG_CONFIG_HOME` 時：`$XDG_CONFIG_HOME/pixtuoid/config.toml`

現行產品只讀五類設定：頂層 `pack-dir`、`[sources]`、`[floating]`、`[audio]` 與 `[characters]`。舊辦公室的 `theme`、`max-desks`、`pets`、`last-seen-version` 等 key 會被忽略，不會重新啟用已移除功能。

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
always-on-top = false

[audio]
muted = true
volume = 0.35
bgm-path = "C:/Music/your-licensed-track.mp3"

[characters]
enabled-slots = [0, 1, 2, 3, 4, 5, 6, 7]
showcase-slots = []
training-showcase-slots = []
```

## 行為

- 視窗預設為 `360 x 240`，可用 `z` 在迷你／小／中／大四段尺寸間切換，並可從任一邊緣或角落自由縮放；最小為 `160 x 96`。
- `always-on-top` 預設為 `false`。按 `t` 或點擊「置頂」字卡可切換；設為 `false` 時使用一般視窗層級，其他應用程式可以蓋住它，回到桌面時仍可看到。已明確儲存成 `true` 的既有設定不會被預設值覆蓋。
- Windows 可按 `a` 或點擊「開機啟動」字卡切換自動啟動。這項選擇使用目前使用者的 Startup 資料夾，不寫進 `config.toml`；程式只會移除自己帶標記建立的入口，遇到同名外部檔案會報錯而不覆寫。
- `opacity` 會限制在 `0.2..1.0`。
- 音訊預設靜音；`volume` 會限制在 `0.0..1.0`。
- `bgm-path` 只讀本機 MP3、WAV、OGG 或 FLAC，不會下載遠端內容。
- `enabled-slots` 是允許分配給真實 Agent 的角色槽位。有效值由目前載入 pack 的完整紙娃娃數動態決定（`0..角色數-1`）；重複或越界值會被移除，空白或全部無效時會安全回到目前 catalog 全開。
- `showcase-slots` 是手動派到自由市場逛街的展示角色。它們頭頂固定標示「逛街中」，不會進入 Agent、工具或父子代理統計；真實 Agent 永遠優先占用市場容量。
- `training-showcase-slots` 是手動派到訓練場練功的展示角色。它們會走傳點、平台與繩索路線，抵達後進入既有攻擊／技能循環，頭頂固定標示「練功中」；同樣不進入 Agent 統計，真實 Agent 永遠優先占用訓練位置。
- 角色面板會以 nearest-neighbor 方式預覽目前 pack 每個 catalog 槽位的實際角色圖，不另外讀取第三方縮圖；每頁八款，可用左右方向鍵或頁面按鈕瀏覽，也可點右上角 `X` 關閉。
- 安裝目錄啟用本機 catalog 輔助程式時，自訂角色可用同列兩次確認刪除；八款必要內建角色不提供刪除。成功後 `enabled-slots`、`showcase-slots` 與 `training-showcase-slots` 會按新索引重新對齊，不會把較後面的角色錯配成別款造型。
- 等待與閒置動作使用 Agent ID、完整動作段落與用途 salt 產生可重現變化；市場會變換停留姿勢與秒數，訓練場會變換左右方向、漫遊距離和休息姿勢。工作中的開店／攻擊語意不受影響。
- CLI 的 `--pack-dir` 優先於設定檔。
- malformed config 只會回到安全預設並發出警告，不會覆寫原檔。

視窗位置、大小、置頂選擇、角色清單、展示角色、靜音和音量會在操作後寫回設定。`sources`、`connect` 與 `disconnect` 會維持既有註解與不認識的 sibling key。

## CLI 與環境變數

```powershell
.\target\debug\maple-agent-market.exe sources
.\target\debug\maple-agent-market.exe doctor
.\target\debug\maple-agent-market.exe floating --pack-dir C:\team\authorized-pack
```

進階顯示調整保留上游相容環境變數：

- `PIXTUOID_LABEL_SCALE`：字卡比例，限制在 `0.75..2.0`。
- `PIXTUOID_FLOATING_SCALE`：整數場景縮放，限制在 `1..8`；預設 `1` 為原生解析度。設為 `2..8` 會先以較低解析度繪製再做 nearest-neighbor 放大，適合刻意換取較低負載，但畫面像素會變大。
- `PIXTUOID_CJK_FONT`：指定本機 CJK 字型檔。
- `PIXTUOID_LOG`：指定 log 檔位置。
- `PIXTUOID_HOOK`：指定 `pixtuoid-hook` 絕對路徑。

這些名稱是 wire / config 相容層，不代表舊 Pixtuoid UI 仍存在。
