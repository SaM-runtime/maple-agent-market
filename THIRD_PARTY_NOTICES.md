# 第三方來源與授權

這份清單只涵蓋 Maple Agent Market 公開 repo 中實際保留的內容。本機 private pack 的界線另見 `FORK_NOTICE.md`。

## Pixtuoid

- 專案：[IvanWng97/pixtuoid](https://github.com/IvanWng97/pixtuoid)
- Fork 基準：`v0.16.0` commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)
- 版權：Copyright (c) 2026 Ivan Wang
- 授權：MIT，完整文字保留在 `LICENSE`
- 實際保留範圍：agent source / hook、event decoder、reducer、場景模擬與路徑、RGB / sprite pack 基礎、相容識別碼，以及 `pixtuoid-core` parser 測試用的小型 `.sprite` fixture

Pixtuoid 的終端 TUI、辦公室 sprite 與範例角色 pack 已從本 fork 移除。公開 snapshot 與內建視窗不再使用 Pixtuoid 辦公室圖像。

再散布本 repo 或其 substantial portions 時，必須保留原作者版權與 MIT permission notice。

## Monaspace Neon

- 專案：[githubnext/monaspace](https://github.com/githubnext/monaspace)
- 版權：Copyright (c) 2023 GitHub
- 授權：SIL Open Font License 1.1
- 檔案：`crates/pixtuoid/fonts/MonaspaceNeon-SemiBold.otf`
- 完整授權：`crates/pixtuoid/fonts/OFL-Monaspace.txt`

## RustCrypto SHA-2

- 專案：[RustCrypto/hashes](https://github.com/RustCrypto/hashes/tree/sha2-v0.11.0/sha2)
- 版本：`sha2 0.11.0`
- 授權：MIT OR Apache-2.0；本專案依 MIT 選項使用與再散布
- 用途：本機素材 manager 的逐檔 SHA-256 與 pack fingerprint
- 完整 MIT 文字：`LICENSES/RustCrypto-SHA2-MIT.txt`

## Maple Atelier 與 maplestory.io（僅本機取得流程）

- Maple Atelier source：[AndyWang505/maple-atelier](https://github.com/AndyWang505/maple-atelier)
- Maple Atelier 授權：其應用程式原始碼採 MIT；本 repo 沒有複製其程式碼，只使用公開 outfit ID／URL 與服務 API
- 角色 render 服務：[maplestory.io](https://maplestory.io/)
- 本 repo 實際保留：`tools` 內本專案自行撰寫的 downloader／converter、固定來源配方與權利提示
- 本 repo 不保留：任一服務回傳的 PNG、GIF、角色影格、遊戲 metadata dump 或衍生 `.sprite`

Maple Atelier 的 README 將角色素材來源標為 maplestory.io，並另行聲明遊戲素材著作權屬 NEXON；maplestory.io 的服務頁也聲明其提供的媒體、圖示、描述與角色資料屬 NEXON。上述站台的程式碼授權不會把 API 回傳的遊戲素材改成 MIT。本專案工具只讓使用者在本機明確同意後重播來源配方，生成結果由使用者自行依原權利條件管理。

## 本專案產生的公開媒體

`assets/readme/maple-agent-market-overview.png` 由目前 `floating_snapshot` example 使用內建程序化 renderer 產生，未載入私人素材或遊戲影格。

內建背景、角色、攤位、怪物、傳送點、泛用 arcane / holy / dragon 效果、command-success flash 與 task-completion pillar 都由本專案 Rust 程式繪製。

## 可稽核 inventory

`policy/public-release/media-allowlist.sha256` 固定已審核媒體的精確 SHA-256；`policy/public-release/media-licences.toml` 將每個媒體路徑對應到來源與授權群組。路徑 pattern 不是授權證明，任何 hash 變更仍需重新審查。

NEXON / MapleStory 紙娃娃、背景、角色、怪物、傳送點、技能、音樂、使用者 skin 與 QA 擷取不屬於此清單，也不隨本 repo 發布。
