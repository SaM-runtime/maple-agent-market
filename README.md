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

- 將執行中、等待、閒置、完成與離場狀態轉換成不同角色行為；非工作段落會依角色與完整動作週期變換停留姿勢、方向、距離與節奏，不會每幀亂跳。
- 角色由傳送點進場，沿平台移動；上層位置會使用梯繩路線，離場時反向返回傳送點。
- 以 `parent_id` 保留主 task 與子代理的群組關係，並在字卡中標示角色層級。
- 角色清單可限制真實 Agent 使用哪些外觀，也可手動派出不代表 Agent 的展示角色到自由市場逛街或訓練場練功。
- 自由市場與訓練場可並列顯示，也能只展開其中一張地圖。
- 浮動視窗支援四段尺寸、八方向拖曳縮放、跨螢幕移動、可切換置頂、Windows 開機啟動、靜音與音量調整。
- 內建程序化地圖、角色、商店框、怪物、傳送點與泛用特效，不依賴外部媒體即可啟動。
- 支援具合法使用權的本機 sprite pack 與 BGM，並提供 pack 驗證及 SHA-256 fingerprint。
- 提供 Codex source 連接、移除與唯讀診斷指令。

### 狀態呈現

| Agent 狀態 | 場景行為 |
|---|---|
| 執行中 | 市場持續開店；訓練場進入攻擊與技能循環 |
| 等待回覆 | 留在角色位置，以坐下／呼吸等完整動作輪替並保留等待提示 |
| 閒置 | 關閉攤位，隨角色與週期改變休息姿勢、漫遊方向、距離與停留時間 |
| 任務完成 | 播放完成特效，保留角色供狀態確認 |
| Session 結束 | 沿場景路線走回傳送點後離場 |
| 子代理 | 與主 task 保持群組關係，字卡顯示對應層級 |

## 快速開始

需求：Git、[Rust](https://www.rust-lang.org/tools/install) 1.89 或更新版本。浮動視窗目前主要在 Windows 驗證。

```powershell
git clone https://github.com/SaM-runtime/maple-agent-market.git C:\dev\maple-agent-market
Set-Location C:\dev\maple-agent-market
cargo build --locked --workspace --bins
.\target\debug\maple-agent-market.exe
```

這個命令會同時建立對外主程式 `maple-agent-market.exe` 與來源整合所需的 `pixtuoid-hook.exe`。`pixtuoid` 是為了保留上游相容契約而維持的內部 crate 名；主程式不帶子命令時會直接開啟浮動視窗，等同：

```powershell
.\target\debug\maple-agent-market.exe floating
```

若要建立與本專案開發版相同結構的本機角色 catalog，可在完成 build 後執行：

```powershell
.\建立本機素材.cmd
# 或明確使用命令列：
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Bootstrap-MapleLocalAssets.ps1 -ProjectRoot $PWD -AcceptThirdPartyAssetNotice -IncludeClassicSkills
.\target\debug\maple-agent-market.exe floating --pack-dir .\private-assets\skins\catalog-pack
```

建立器先產生 8 款本專案原創程序化角色，再依版本化配方在使用者電腦下載 19 款 Maple Atelier 公開 outfit 與 1 款固定 TWMS 造型，共 28 款角色；友善啟動器也會重建 GMS v62（BB 前）的天怒與龍咆嘯完整時間軸。下載結果只會寫入 Git 已忽略的 `private-assets`。API 內容可能隨上游更新或下架，因此「相同配方與功能」可重現，但第三方服務無法保證永久回傳逐位元相同的檔案。只想離線驗證工具時可加 `-StarterOnly`，不會發出網路請求；若命令列模式不想抓技能，可省略 `-IncludeClassicSkills`。

### 可以重現到什麼程度

| 項目 | 乾淨 clone 後的結果 |
|---|---|
| Rust 引擎、Codex 監控、雙地圖、角色清單、展示／練功、即時匯入與刪除、開機啟動、縮放與置頂 | 與這個 repository 的相同 commit 一致 |
| 28 款角色的 catalog 結構 | 一致：8 款 MIT 程序化 starter 加 20 款由固定公開配方在本機取得的造型 |
| BB 前天怒與龍咆嘯的技能時間軸 | 可由工具在本機重建；服務回應可能隨上游變動 |
| 作者電腦上曾使用的八款 ranking 紙娃娃 | 不公開；以八款原創 starter 取代 |
| 作者電腦上的自由市場截圖底圖、弓箭手村衍生圖、怪物／傳點影格、Magic Claw 與 BGM | 不公開也不由 bootstrap 偽裝成可再散布內容；程式會使用公開的程序化 fallback，使用者可再載入自己有權使用的本機素材 |

因此，公開版提供的是相同的功能基底、28 槽角色工作流與可重播配方，不宣稱能在第三方素材缺乏再散布授權時，仍逐像素複製作者的私人安裝畫面。

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
| `z` | 循環切換迷你 / 小 / 中 / 大尺寸 |
| `t` | 切換永遠置頂；關閉後可留在桌面、由其他視窗自然覆蓋 |
| `c` | 開啟／關閉角色清單與展示角色控制面板；面板開啟時也可點右上角 `X` 關閉 |
| `←` / `→` | 在角色面板中切換上一頁／下一頁 |
| `a` | 切換 Windows 開機自動啟動；只管理本程式建立的 Startup 入口 |
| `m` | 靜音 / 取消靜音 |
| `+` / `-` | 調整音量 |
| 左鍵拖曳 | 移動無邊框視窗 |
| 任一邊緣或角落拖曳 | 自由縮放；游標會提示縮放方向 |

地圖、大小、置頂與角色字卡都可以直接點擊。視窗可縮到 `160 × 96`；在極小尺寸下控制字卡會逐步隱藏，但 `t`、`c` 等快捷鍵仍可使用。

角色面板每列會直接預覽目前已載入 sprite pack 的角色圖，名稱與造型可在同一列辨識；角色超過八款時會自動分頁，可點「上一頁／下一頁」或按方向鍵瀏覽完整 catalog。角色名稱左側的綠勾代表「允許真實 Agent 使用這款造型」，不是派角色登場；系統至少保留一款，且同一 Agent 的外觀會跨地圖保持穩定。每列右側另有「逛街」與「練功」按鈕：前者從自由市場傳點進場並顯示「逛街中」，後者從訓練場傳點進場、沿平台／繩索抵達位置後進入攻擊與技能循環，頭頂只顯示「練功中」。再次點擊會撤回角色。兩種展示都不會被算入 Agent 數量、工具統計或父子代理關係；每張地圖最多顯示八位角色，滿員時永遠優先保留真實 Agent。安裝目錄有啟用本機 catalog 輔助程式時，自訂角色另會顯示「刪除」；必須在同一列再按一次「確認」才會執行，八款必要內建角色不提供刪除。

`a` 會在目前 Windows 使用者的 Startup 資料夾建立或移除一個帶有專案標記的啟動入口，重播當前執行檔、參數及必要的本機路徑環境。預設關閉；如果同名檔案不是由 Maple Agent Market 建立，程式會拒絕覆寫或刪除。

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

公開 repo 與公開 binary bundle 會提供 `tools/Import-MapleAtelierCharacter.ps1`、本機素材工作坊及版本化來源配方，但不提供任何下載結果。先在 [Maple Atelier](https://maple-atelier.org/simulator) 完成造型並按「複製」，再於角色面板點「＋ 新增造型（剪貼簿）」。輔助程式只接受 Maple Atelier 公開 outfit、`/simulator?load=` 或其複製出的 `maplestory.io` HTTPS 角色渲染網址；裸 `/simulator` 網址不含造型資料，因此不接受。角色面板會持續顯示匯入進度；成功後會在同一個執行中的視窗重新載入 catalog、跳到新角色所在頁並顯示結果，不必重新啟動程式。自訂角色的雙重確認刪除也會即時重載 catalog，並把較後面的 Agent／逛街／練功選擇向前對齊；輔助程式會先把被移除的本機來源移到 `deleted` 目錄，重建失敗時則還原原 catalog 與來源。取消或失敗都會保留繁中原因，且同一時間只允許一個 catalog 變更工作。為避免覆蓋團隊或其他自訂 pack，熱匯入與刪除只在目前以明確 `--pack-dir` 載入同一個 `private-assets/skins/catalog-pack` 時啟用。

這個本機流程產生的紙娃娃、技能與其他第三方影格仍是 NEXON-derived 素材，不會因為建立器本身採 MIT 就取得再散布授權。公開的是下載／轉換程式、來源 ID 與配方；生成的 PNG、GIF、`.sprite`、catalog pack 和來源快取永遠留在 Git 已忽略的本機目錄。Maple Atelier 的 README 也把角色素材來源標示為 `maplestory.io`，而 maplestory.io 本身聲明所提供的媒體仍屬 NEXON；因此本專案不把那些回應宣告為 MIT。

本機 BGM 可在相容設定檔 `~/.config/pixtuoid/config.toml` 指向你有權播放的 MP3、WAV、OGG 或 FLAC：

```toml
[audio]
muted = true
volume = 0.35
bgm-path = "C:/Music/your-licensed-track.mp3"
```

程式第一次啟動時預設靜音；需要播放時可在視窗內按 `M` 解除靜音。專案不含 YouTube downloader、內嵌 web player 或音樂檔。

## 專案結構

| 路徑 | 用途 |
|---|---|
| `crates/pixtuoid` | CLI、來源連接、浮動視窗與音訊入口 |
| `crates/pixtuoid-core` | Agent 事件、狀態 reducer、來源解碼與 session 監看 |
| `crates/pixtuoid-scene` | 地圖配置、角色動作、標籤、程序化 renderer 與特效 |
| `crates/pixtuoid-hook` | 供支援來源呼叫的輕量 hook 執行檔 |
| `docs` | 架構、設定、素材協作與發布政策 |
| `scripts` | 公開發布稽核與建置工具 |
| `tools` | 可公開的本機素材建立、Maple Atelier 匯入、角色配方與 BB 前技能同步工具；生成結果不進 Git |

更完整的設計說明見 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) 與 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。

## 素材與發布邊界

Repository 只包含具有再散布依據的內容：

| 內容 | 來源 / 授權 |
|---|---|
| Maple Agent Market 修改、程序化 UI 與特效 | 本專案貢獻者，MIT |
| 保留的 Pixtuoid 核心程式與小型 parser 測試 fixture | Ivan Wang / Pixtuoid，MIT |
| Monaspace Neon 字型 | GitHub Next，SIL OFL 1.1 |

下列內容不會隨 repository、release、binary、README 圖片或 CI artifact 發布；repo 只提供產生它們的程式與來源配方：

- NEXON / MapleStory 的背景、角色、怪物、傳送點、商店框、技能影格或音樂；
- 遊戲截圖、WZ / client 拆包、Open API 紙娃娃及其裁切或轉檔衍生物；
- 使用者的皮膚、BGM、private pack、cache 或本機測試截圖。

Repository 的可見性、用途是否營利或使用人數，都不會自動產生第三方素材的再散布授權。詳細政策見 [`FORK_NOTICE.md`](FORK_NOTICE.md) 與 [`docs/OPEN_SOURCE_RELEASE.md`](docs/OPEN_SOURCE_RELEASE.md)。這是保守的工程發布政策，不是法律意見。

提交前可執行：

```powershell
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

### 可重現的公開 bundle

`maple-agent-market` 是浮動視窗；`pixtuoid-hook` 是連接 Codex 等來源時使用的 hook shim。公開發行必須同時保留兩個執行檔，不能只複製前者。下列命令會以路徑重寫編譯兩者，並各自做 binary 稽核：

```powershell
python scripts\build-public-release.py
```

在乾淨、已審核並已 commit 的 working tree，可將兩個執行檔、授權與 SHA-256 清單放入 repository 外的新資料夾；既有輸出路徑會被拒絕覆寫：

```powershell
python scripts\stage-public-release.py --output C:\dist\maple-agent-market-0.16.0
```

bundle 的 `PUBLIC_BUNDLE_MANIFEST.json` 會保留 `entrypoint`（主程式）供既有工具使用，並以 `entrypoints` 列出 `maple-agent-market(.exe)` 與 `pixtuoid-hook(.exe)`；同時帶入 `tools` 內的本機素材建立器與配方，但仍明示 `contains_private_maple_assets: false`。`SHA256SUMS.txt` 會遞迴涵蓋其自身以外的每個 bundle 檔案。

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
