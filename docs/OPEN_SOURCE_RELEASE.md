# 公開發布邊界

這份文件定義 Maple Agent Market 可上傳 GitHub 與提供朋友 clone 的內容。它用來避免本機 fan presentation 與 MIT 原始碼被誤當成同一套可再散布資產。

## 公開 repo 會包含

- Maple Agent Market 與 Pixtuoid 衍生的 MIT Rust／Python／PowerShell 原始碼；
- Pixtuoid MIT 預設像素素材與 pack 格式範本；
- Monaspace Neon 字型與完整 SIL OFL 1.1；
- 原創程式化技能、命令成功與 task 完成效果；
- 公開安全版 README 截圖、測試與素材稽核工具。

## 公開 repo 不會包含

- NEXON／MapleStory 背景、角色、怪物、傳送點、商店框、技能影格與 BGM；
- 遊戲截圖、WZ／client 拆包檔、API 紙娃娃及其裁切／轉檔衍生物；
- 使用者 skin、active pack、preview、cache、QA 截圖或本機音樂；
- Codex transcript、憑證、個人絕對路徑、archive、installer 或已編譯的開發產物。

朋友乾淨 clone 後可以直接建置與開啟公開安全版；要呈現另一套視覺，可自行在本機以 `--pack-dir` 載入有權使用的素材。沒有私人 pack 時不會重現維護者本機的 NEXON 風格畫面。

## 自動稽核

```powershell
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

稽核涵蓋 tracked 與未被 `.gitignore` 排除的 untracked 檔案，會拒絕私人素材路徑、未核准媒體、音訊／封裝／binary、常見秘密字串與目前機器的絕對路徑。輸出不會印出命中的秘密內容。

`policy/public-release/media-allowlist.sha256` 是已審核檔案的精確 byte inventory；`media-licences.toml` 必須把每個媒體檔對應到唯一的授權來源。hash 改變時不能只為了讓 audit 通過而更新，必須重新檢查來源與畫面。

## 建立公開 binary

```powershell
python scripts\build-public-release.py --selftest
python scripts\build-public-release.py
```

這條路徑會啟用編譯路徑 remap，並再次掃描成品，避免把維護者的 home／workspace 路徑嵌入 binary。

已提交且 worktree 乾淨後，可建立不覆寫目的地的完整 bundle：

```powershell
python scripts\stage-public-release.py --output C:\temp\maple-agent-market-public
```

bundle 只包含經稽核的執行檔、MIT／OFL notices、`PUBLIC_BUNDLE_MANIFEST.json` 與 `SHA256SUMS.txt`，不會自動複製本機 private pack。

## 發布前清單

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo test --locked --workspace`
- [ ] public release audit 與其 self-test 通過
- [ ] README 的 clone／啟動指令已從乾淨目錄實測
- [ ] 所有媒體都有 notice、license group 與正確 SHA-256
- [ ] `git status` 只有準備提交的檔案
- [ ] `origin` 指向 `SaM-runtime/maple-agent-market`，`upstream` 指向 Pixtuoid

這是保守的工程發布邊界，不是法律意見。沒有取得涵蓋 GitHub source／binary 再散布的明確書面授權前，不把遊戲素材視為 MIT 內容。
