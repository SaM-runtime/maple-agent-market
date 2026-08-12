# 設定

Maple Agent Market 沿用 Pixtuoid 的設定位置：

- Windows：`%USERPROFILE%\.config\pixtuoid\config.toml`
- macOS／Linux：`~/.config/pixtuoid/config.toml`，並遵守 `XDG_CONFIG_HOME`

所有欄位都可省略。CLI 參數優先於設定檔。

## 建議起始設定

```toml
theme = "maple"
# pack-dir = "C:/Users/you/.config/pixtuoid/packs/my-local-pack"

[floating]
width = 1280
height = 560

[audio]
muted = true
volume = 0.35
# bgm-path = "C:/Music/your-licensed-track.mp3"
```

`[floating]` 的位置與尺寸會在關閉視窗時更新。太小的數值會被夾到安全下限；也可在視窗內按 `z` 切換尺寸預設。

## 常用欄位

| 欄位 | 說明 |
|---|---|
| `theme = "maple"` | 使用 Maple Agent Market 雙地圖主題 |
| `pack-dir` | 使用者自己的本機 sprite pack；支援 `~` 展開 |
| `max-desks` | TUI 每層最多座位數；floating 會依視窗大小配置 |
| `[floating].width/height/x/y` | 浮動視窗幾何資訊 |
| `[audio].muted` | 是否靜音；可在視窗按 `m` 切換 |
| `[audio].volume` | `0.0` 到 `1.0`；可按 `+`／`-` 調整 |
| `[audio].bgm-path` | 本機 MP3、WAV、OGG 或 FLAC；不支援 YouTube 下載或串流 |
| `[sources]` | `connect`／`disconnect` 寫入的 agent source 開關 |

`last-seen-version` 也是系統管理欄位，用來避免同一版更新訊息重複出現，不建議手動修改。

## 自訂 sprite pack

```powershell
.\target\debug\pixtuoid.exe init-pack .\my-local-pack
.\target\debug\pixtuoid.exe validate-pack .\my-local-pack
.\target\debug\pixtuoid.exe --theme maple floating --pack-dir .\my-local-pack
```

repo 內的 `crates/pixtuoid/sprites/skeleton/` 是格式範本。使用者素材、轉換結果、preview、cache 與 BGM 必須放在 repo 外，除非它們具有可驗證的公開再散布授權。

Windows 也提供 `tools/windows/MapleSkinWorkshop.psm1` 協助建立與驗證本機角色 pack；其測試位於 `tools/windows/tests/`。

## 素材管理器

```powershell
.\target\debug\pixtuoid.exe assets list
.\target\debug\pixtuoid.exe assets install public-classic
.\target\debug\pixtuoid.exe assets import C:\team\authorized-pack --id team-pack
.\target\debug\pixtuoid.exe assets verify team-pack --expect <64位SHA-256>
```

managed packs 預設位於設定目錄下的 `packs/`；所有子命令都可在 `assets` 後加上 `--root <PATH>` 使用隔離位置。安裝／匯入結果會印出可直接傳給 `--pack-dir` 的路徑。完整協作流程見 [`ASSET_COLLABORATION.md`](ASSET_COLLABORATION.md)。

## 診斷

```powershell
.\target\debug\pixtuoid.exe sources
.\target\debug\pixtuoid.exe doctor
```

一般紀錄位於 `~/.cache/pixtuoid/log`，crash 紀錄位於 `~/.cache/pixtuoid/crash.log`。可用 `PIXTUOID_LOG` 指定其他 log 路徑。
