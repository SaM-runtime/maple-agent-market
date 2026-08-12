<h1 align="center">Maple Agent Market</h1>

<p align="center">
  把 Codex 與其他 coding agents 的工作狀態，呈現在會擺攤、移動、等待與練功的 2D 浮動視窗。
</p>

<p align="center">
  <strong>非官方社群 side project</strong> · Windows 優先 · Rust · 本機運作 · 無遙測
</p>

![Maple Agent Market 內建雙地圖畫面](assets/readme/maple-agent-market-overview.png)

> 這張圖由目前原始碼的 `floating_snapshot` 直接渲染。沒有載入私人素材包，也沒有使用遊戲截圖、拆包圖像或遊戲音樂。朋友乾淨 clone 後即可編譯出同一套內建畫面。

## 這是什麼

Maple Agent Market 是桌面上的 agents orchestration visualizer。每個本機 agent session 會被映射成一名 2D 角色，讓你看出誰正在執行、等待回覆、閒置或離場；來源若提供可靠的 `parent_id`，畫面也會呈現主 task 與子代理關係。

目前有兩張可共存的地圖：

- 市場：工作中的 agent 開店，等待或閒置時切換為相應狀態。
- 訓練場：以走路、爬梯、怪物巡邏和泛用程式化技能呈現活動。

本程式只讀取本機 agent 事件與 transcript，不會控制 agent，也不會上傳 session 內容。

## 乾淨 clone 會得到什麼

- 原創的程序化雙地圖、角色、商店框、怪物、傳送點與泛用特效。
- 浮動視窗、狀態映射、進退場、角色移動、主從 task 標示與訊息列。
- Codex 與其他已支援 agent source 的 hook、事件解碼和診斷工具。
- 可選的本機 sprite pack 匯入與 SHA-256 驗證介面。

內建畫面不依賴外部圖片包，因此不需要 `assets install`、下載器或私有素材才能開啟。不同電腦上的視窗尺寸、DPI、時間與實際 session 狀態可能讓即時畫面不同；相同 commit 的固定 snapshot 則可重現。

## 快速開始

需求：Git、[Rust](https://www.rust-lang.org/tools/install) 1.89 或更新版本。Maple 浮動視窗目前主要在 Windows 驗證。

```powershell
git clone https://github.com/SaM-runtime/maple-agent-market.git C:\dev\maple-agent-market
Set-Location C:\dev\maple-agent-market
cargo build --locked -p pixtuoid
.\target\debug\maple-agent-market.exe
```

`pixtuoid` 是為了保留上游相容契約而維持的內部 crate 名；使用者執行檔是 `maple-agent-market.exe`。不帶子命令時會直接開啟浮動視窗，等同：

```powershell
.\target\debug\maple-agent-market.exe floating
```

### 連接 Codex

先查看可用來源與目前狀態：

```powershell
.\target\debug\maple-agent-market.exe sources
```

連接 Codex 會在 Codex 的本機設定中安裝此專案的 hook；這是明確的本機設定變更：

```powershell
.\target\debug\maple-agent-market.exe connect codex
```

要移除時執行：

```powershell
.\target\debug\maple-agent-market.exe disconnect codex
```

若角色沒有出現，可執行唯讀診斷：

```powershell
.\target\debug\maple-agent-market.exe doctor
```

### 浮動視窗操作

| 操作 | 功能 |
|---|---|
| `Esc` | 關閉目前聚焦的浮動視窗 |
| `1` / `2` / `3` | 雙地圖 / 市場 / 訓練場 |
| `Tab` | 循環切換地圖 |
| `z` | 循環切換小 / 中 / 大尺寸 |
| `m` | 靜音 / 取消靜音 |
| `+` / `-` | 調整音量 |
| 左鍵拖曳 | 移動無邊框視窗 |
| 右下角拖曳 | 自由縮放 |

## 自訂本機素材

公開版不需要素材包。若你擁有可合法使用的 sprite pack，可先驗證，再以 `--pack-dir` 載入：

```powershell
.\target\debug\maple-agent-market.exe validate-pack C:\team\authorized-pack
.\target\debug\maple-agent-market.exe floating --pack-dir C:\team\authorized-pack
```

也可以匯入到本機管理目錄並比對 fingerprint：

```powershell
.\target\debug\maple-agent-market.exe assets import C:\team\authorized-pack --id team-pack
.\target\debug\maple-agent-market.exe assets verify team-pack --expect <64位SHA-256>
.\target\debug\maple-agent-market.exe assets list
```

匯入只會複製使用者指定的本機 pack，不會下載、上傳或改變素材授權。協作方式見 [`docs/ASSET_COLLABORATION.md`](docs/ASSET_COLLABORATION.md)。

本機 BGM 可在相容設定檔 `~/.config/pixtuoid/config.toml` 指向你有權播放的 MP3、WAV、OGG 或 FLAC：

```toml
[audio]
muted = false
volume = 0.35
bgm-path = "C:/Music/your-licensed-track.mp3"
```

專案不含 YouTube downloader、內嵌 web player 或音樂檔。

## 公開素材邊界

GitHub 版只包含可再散布的內容：

| 內容 | 來源 / 授權 |
|---|---|
| Maple Agent Market 修改、程序化 UI 與特效 | 本專案貢獻者，MIT |
| 保留的 Pixtuoid 核心程式與小型 parser 測試 fixture | Ivan Wang / Pixtuoid，MIT |
| Monaspace Neon 字型 | GitHub Next，SIL OFL 1.1 |

下列內容不會隨 repo、release、binary、README 截圖或 CI artifact 發布：

- NEXON / MapleStory 的背景、角色、怪物、傳送點、商店框、技能影格或音樂；
- 遊戲截圖、WZ / client 拆包、Open API 紙娃娃及其裁切或轉檔衍生物；
- 使用者的皮膚、BGM、private pack、cache 或本機測試截圖。

GitHub 設為 public 或 private、只有朋友能 clone、以及是否營利，都不會自動產生再散布授權。詳細邊界見 [`FORK_NOTICE.md`](FORK_NOTICE.md) 與 [`docs/OPEN_SOURCE_RELEASE.md`](docs/OPEN_SOURCE_RELEASE.md)。這是保守的工程發布政策，不是法律意見。

可在提交前執行：

```powershell
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

## 與 Pixtuoid 的關係

本專案是 [IvanWng97/pixtuoid](https://github.com/IvanWng97/pixtuoid) 的 source-code fork，基準為 Pixtuoid `v0.16.0` commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)。本 repo 保留 Ivan Wang 的 MIT 版權與授權聲明。

目前仍依賴的上游部分包括 agent event / source / hook 架構、reducer、場景模擬與路徑資料、RGB buffer / sprite pack 基礎，以及既有相容識別碼。這些不是只改名就能刪除的舊檔；浮動視窗會直接呼叫它們。

已從本 fork 移除的上游產品面包括終端 TUI、舊辦公室 sprite、robot / skeleton 範例 pack、`run`、`init-pack`、`public-classic` 安裝器，以及上游 Homebrew completion / man-page 產生器。乾淨 clone 不再顯示 Pixtuoid 辦公室素材。

更多範圍與逐項來源見 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) 與 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。

## 共同開發

```powershell
git switch -c feature/my-change
cargo fmt --all -- --check
cargo test --locked --workspace
python scripts\public-release-audit.py
```

完整 gate 為 `just preflight`。架構、設定與貢獻規則分別在 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)、[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) 與 [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)。

## 非官方聲明與授權

本專案未受 NEXON、遊戲橘子、OpenAI、Pixtuoid 維護者或其他第三方贊助、認可或背書。MapleStory、NEXON 與相關名稱、角色及標誌屬各自權利人；專案名稱中的「Maple」只描述非官方懷舊 2D 主題。

程式碼依 [`LICENSE`](LICENSE) 中的 MIT License 發布。第三方內容仍依各自授權；本機素材不會因載入本程式而改為 MIT。
