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

## 本專案產生的公開媒體

`assets/readme/maple-agent-market-overview.png` 由目前 `floating_snapshot` example 使用內建程序化 renderer 產生，未載入私人素材或遊戲影格。

內建背景、角色、攤位、怪物、傳送點、泛用 arcane / holy / dragon 效果、command-success flash 與 task-completion pillar 都由本專案 Rust 程式繪製。

## 可稽核 inventory

`policy/public-release/media-allowlist.sha256` 固定已審核媒體的精確 SHA-256；`policy/public-release/media-licences.toml` 將每個媒體路徑對應到來源與授權群組。路徑 pattern 不是授權證明，任何 hash 變更仍需重新審查。

NEXON / MapleStory 紙娃娃、背景、角色、怪物、傳送點、技能、音樂、使用者 skin 與 QA 擷取不屬於此清單，也不隨本 repo 發布。
