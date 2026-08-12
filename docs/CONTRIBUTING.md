# 參與開發

感謝一起改進 Maple Agent Market。這個 repo 專注於可直接建置的 Rust 視覺化程式，不包含舊版官網、npm launcher、Raycast extension 或本機私人素材包。

## 開始

```powershell
git clone https://github.com/SaM-runtime/maple-agent-market.git C:\dev\maple-agent-market
Set-Location C:\dev\maple-agent-market
git switch -c feature/my-change
cargo build --locked -p pixtuoid
```

內部 binary／crate 仍叫 `pixtuoid`，這是為了維持設定、hook 與來源相容性；面向使用者的名稱是 Maple Agent Market。

## 修改原則

- `pixtuoid-core` 與 `pixtuoid-scene` 不依賴 terminal／window backend。
- source 只從公開事件或唯讀 transcript 建立狀態；不要把推測寫成 parent/subagent 關係。
- 工作中的角色保持穩定狀態；動畫用來表達真正的狀態轉換，不要造成反覆開關店的假訊號。
- 新增畫面行為時，優先補公開 renderer 的 deterministic 測試或 snapshot seam。
- 不要提交本機 session、絕對路徑、private pack、cache、BGM、遊戲截圖或無再散布授權的素材。

## 提交前驗證

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

已安裝 `just` 時可改用：

```powershell
just preflight
```

修改畫面後可重拍公開安全版 README 圖：

```powershell
just snapshot
```

這個命令不可搭配私人 pack；提交前要目視確認圖片沒有 NEXON／MapleStory 素材、私密 task 文字或個人路徑。

## 第三方素材

只有具備明確再散布授權的檔案才能進入 repo。新增或替換媒體時必須同時：

1. 在 `THIRD_PARTY_NOTICES.md` 記錄作者、來源 URL、授權與修改方式。
2. 在 `policy/public-release/media-licences.toml` 指派唯一的來源群組。
3. 經來源與視覺審查後，更新 `media-allowlist.sha256` 的精確 hash。
4. 執行公開發布 audit。

單純把檔案放在網頁、API 或遊戲 client 中，不代表它具有開源再散布授權。

共同修改公開 sprite 後，重新 build 並執行 `assets install public-classic --force`；在 PR 描述附上 `assets verify public-classic` 的新 fingerprint，讓 reviewer 可重現逐檔內容。

`assets import` 產生的 pack 固定標為 `local-only`；除非已另行完成作者、來源與再散布授權審查，不能把 managed install 反向複製進 Git。完整流程見 [`ASSET_COLLABORATION.md`](ASSET_COLLABORATION.md)。

## Pull request

PR 請說明問題、視覺／狀態影響與實際執行的驗證。安全性問題不要開公開 issue，請依 `SECURITY.md` 使用 GitHub private vulnerability reporting。
