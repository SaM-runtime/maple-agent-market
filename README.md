<h1 align="center">Maple Agent Market</h1>

<p align="center">
  將本機 coding agents 的執行、等待、閒置與子代理關係，映射成 2D MMORPG 風格的桌面場景。
</p>

<p align="center">
  <strong>非官方社群 side project</strong> · Windows 優先 · Rust · 本機運作 · 無遙測
</p>

![Maple Agent Market 雙地圖實機畫面](assets/readme/maple-agent-market-overview.png)

_市場與訓練場可並列顯示；畫面由 Maple Agent Market 的即時 renderer 產生。_

## 專案概覽

Maple Agent Market 是桌面上的 agents orchestration visualizer。它把 Codex 與其他已支援來源的本機 agent session 轉換成場景中的角色，讓執行狀態、等待點、父子代理關係和進退場過程一眼可見。

目前包含兩種可獨立或並列顯示的場景：

- **自由市場**：工作中的 agent 會開設攤位；商店框顯示短任務名稱、來源與父子關係。
- **訓練場**：角色會走路、爬梯、攻擊巡邏怪物，並依工作週期輪替泛用技能特效。

應用程式只讀取本機 agent 事件與 transcript，不會控制 agent，也不會上傳 session 內容。

## 核心功能

- 將執行中、等待、閒置、完成與離場狀態轉換成不同角色行為。
- 角色由傳送點進場，沿平台移動；上層位置會使用梯繩路線，離場時反向返回傳送點。
- 以 `parent_id` 保留主 task 與子代理的群組關係，並在字卡中標示角色層級。
- 自由市場與訓練場可並列顯示，也能只展開其中一張地圖。
- 浮動視窗支援三段尺寸、自由縮放、跨螢幕拖曳、靜音與音量調整。
- 內建程序化地圖、角色、商店框、怪物、傳送點與泛用特效，不依賴外部媒體即可啟動。
- 支援具合法使用權的本機 sprite pack 與 BGM，並提供 pack 驗證及 SHA-256 fingerprint。
- 提供 Codex source 連接、移除與唯讀診斷指令。

### 狀態呈現

| Agent 狀態 | 場景行為 |
|---|---|
| 執行中 | 市場持續開店；訓練場進入攻擊與技能循環 |
| 等待回覆 | 停留在角色位置，顯示等待提示與任務字卡 |
| 閒置 | 關閉攤位，在可行走區域內漫遊 |
| 任務完成 | 播放完成特效，保留角色供狀態確認 |
| Session 結束 | 沿場景路線走回傳送點後離場 |
| 子代理 | 與主 task 保持群組關係，字卡顯示對應層級 |

## 快速開始

需求：Git、[Rust](https://www.rust-lang.org/tools/install) 1.89 或更新版本。浮動視窗目前主要在 Windows 驗證。

```powershell
git clone https://github.com/SaM-runtime/maple-agent-market.git C:\dev\maple-agent-market
Set-Location C:\dev\maple-agent-market
cargo build --locked -p pixtuoid
.\target\debug\maple-agent-market.exe
```

`pixtuoid` 是為了保留上游相容契約而維持的內部 crate 名；對外執行檔是 `maple-agent-market.exe`。不帶子命令時會直接開啟浮動視窗，等同：

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

移除連接：

```powershell
.\target\debug\maple-agent-market.exe disconnect codex
```

角色未出現時，可執行唯讀診斷：

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

若你擁有可合法使用的 sprite pack，可先驗證，再以 `--pack-dir` 載入：

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

匯入只會複製使用者指定的本機 pack，不會下載、上傳或改變素材授權。Pack 格式與協作流程見 [`docs/ASSET_COLLABORATION.md`](docs/ASSET_COLLABORATION.md)。

本機 BGM 可在相容設定檔 `~/.config/pixtuoid/config.toml` 指向你有權播放的 MP3、WAV、OGG 或 FLAC：

```toml
[audio]
muted = false
volume = 0.35
bgm-path = "C:/Music/your-licensed-track.mp3"
```

專案不含 YouTube downloader、內嵌 web player 或音樂檔。

## 專案結構

| 路徑 | 用途 |
|---|---|
| `crates/pixtuoid` | CLI、來源連接、浮動視窗與音訊入口 |
| `crates/pixtuoid-core` | Agent 事件、狀態 reducer、來源解碼與 session 監看 |
| `crates/pixtuoid-scene` | 地圖配置、角色動作、標籤、程序化 renderer 與特效 |
| `crates/pixtuoid-hook` | 供支援來源呼叫的輕量 hook 執行檔 |
| `docs` | 架構、設定、素材協作與發布政策 |
| `scripts` | 公開發布稽核與建置工具 |

更完整的設計說明見 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) 與 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。

## 素材與發布邊界

Repository 只包含具有再散布依據的內容：

| 內容 | 來源 / 授權 |
|---|---|
| Maple Agent Market 修改、程序化 UI 與特效 | 本專案貢獻者，MIT |
| 保留的 Pixtuoid 核心程式與小型 parser 測試 fixture | Ivan Wang / Pixtuoid，MIT |
| Monaspace Neon 字型 | GitHub Next，SIL OFL 1.1 |

下列內容不會隨 repository、release、binary、README 圖片或 CI artifact 發布：

- NEXON / MapleStory 的背景、角色、怪物、傳送點、商店框、技能影格或音樂；
- 遊戲截圖、WZ / client 拆包、Open API 紙娃娃及其裁切或轉檔衍生物；
- 使用者的皮膚、BGM、private pack、cache 或本機測試截圖。

Repository 的可見性、用途是否營利或使用人數，都不會自動產生第三方素材的再散布授權。詳細政策見 [`FORK_NOTICE.md`](FORK_NOTICE.md) 與 [`docs/OPEN_SOURCE_RELEASE.md`](docs/OPEN_SOURCE_RELEASE.md)。這是保守的工程發布政策，不是法律意見。

提交前可執行：

```powershell
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

## 與 Pixtuoid 的關係

本專案源自 [IvanWng97/pixtuoid](https://github.com/IvanWng97/pixtuoid)，基準為 Pixtuoid `v0.16.0` commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)。Repository 保留 Ivan Wang 的 MIT 版權與授權聲明。

Maple Agent Market 延用並擴充上游的 agent event / source / hook 架構、reducer、場景模擬、路徑資料、RGB buffer 與 sprite pack 基礎。這些元件仍是目前 runtime 的一部分，因此內部 crate、設定鍵與部分相容識別碼繼續沿用 `pixtuoid` 名稱。

目前產品介面已移除上游終端 TUI、舊辦公室 sprite、robot / skeleton 範例 pack、`run`、`init-pack`、`public-classic` 安裝器，以及 Homebrew completion / man-page 產生器。

逐項來源與授權見 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 開發與驗證

```powershell
git switch -c feature/my-change
cargo fmt --all -- --check
cargo test --locked --workspace
python scripts\public-release-audit.py
```

完整 gate 為 `just preflight`。貢獻規則見 [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)。

## 非官方聲明與授權

本專案未受 NEXON、遊戲橘子、OpenAI、Pixtuoid 維護者或其他第三方贊助、認可或背書。MapleStory、NEXON 與相關名稱、角色及標誌屬各自權利人；專案名稱中的「Maple」只描述非官方懷舊 2D 主題。

程式碼依 [`LICENSE`](LICENSE) 中的 MIT License 發布。第三方內容仍依各自授權；本機素材不會因載入本程式而改為 MIT。
