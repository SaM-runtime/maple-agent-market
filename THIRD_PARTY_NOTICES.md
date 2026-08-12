# 第三方來源與授權

這份清單只涵蓋 Maple Agent Market 公開 repo 中實際提供的內容。本機 private pack 的界線另見 `FORK_NOTICE.md`。

## Pixtuoid

- 專案：[IvanWng97/pixtuoid](https://github.com/IvanWng97/pixtuoid)
- Fork 基準：`v0.16.0`，commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)
- 版權：Copyright (c) 2026 Ivan Wang
- 授權：MIT，完整文字保留在 `LICENSE`
- 使用範圍：基礎程式、agent source／hook 架構、renderer、相容識別碼、預設像素素材與測試 fixture

再散布本 repo 或其 substantial portions 時，必須保留原作者版權與 MIT permission notice。

## Monaspace Neon

- 專案：[githubnext/monaspace](https://github.com/githubnext/monaspace)
- 版權：Copyright (c) 2023 GitHub
- 授權：SIL Open Font License 1.1
- 檔案：`crates/pixtuoid/fonts/MonaspaceNeon-SemiBold.otf`
- 完整授權：`crates/pixtuoid/fonts/OFL-Monaspace.txt`

## 本專案產生的公開媒體

`assets/readme/maple-agent-market-overview.png` 由目前的 `floating_snapshot` example 使用公開 renderer 與 Pixtuoid MIT 預設 pack 產生，未載入私人素材，也不是遊戲擷取畫面。

arcane claw fallback、holy light、dragon pulse、command-success flash 與 task-completion pillar 是 Rust 原始碼程式繪製的泛用像素效果，沒有內嵌第三方遊戲影格。

## 可稽核 inventory

`policy/public-release/media-allowlist.sha256` 固定已審核媒體的精確 SHA-256；`policy/public-release/media-licences.toml` 將每個媒體路徑對應到唯一的來源／授權群組。路徑符合 pattern 本身不是授權證明，任何 hash 變更仍需重新進行來源與視覺審查。

NEXON／MapleStory 紙娃娃、背景、角色、怪物、傳送點、技能、音樂、使用者 skin 與 QA 擷取不屬於此清單，也不隨本 repo 發布。
