# 素材協作

## 內建版本

乾淨 clone 不需要下載或安裝素材。雙地圖、角色、攤位、怪物、傳送點與泛用技能由 Rust 程序化繪製；相同 commit 可直接 build / run。

```powershell
cargo build --locked --workspace --bins
.\target\debug\maple-agent-market.exe
```

上述建置同時產生浮動視窗 `maple-agent-market.exe` 與來源整合所需的 `pixtuoid-hook.exe`。

`assets list` 在沒有匯入任何本機 pack 時回傳空清單，這是正常狀態，不代表畫面缺少檔案。

## 合法自訂 pack

只在你對全部檔案具備使用與分享權時，才把 pack 提供給共同開發者。匯入不會上傳內容，也不會把素材改成 MIT。

```powershell
.\target\debug\maple-agent-market.exe validate-pack C:\team\authorized-pack
.\target\debug\maple-agent-market.exe assets import C:\team\authorized-pack --id team-pack
.\target\debug\maple-agent-market.exe assets verify team-pack
```

`assets import` 會：

1. 只讀 `pack.toml` 引用的 regular files；
2. 拒絕 symlink、路徑逸出與不受管理的覆寫；
3. 寫入 `ASSET-MANIFEST.json`；
4. 對排序後的完整檔案 inventory 計算 SHA-256 fingerprint。

共同開發者收到同一份合法 pack 後，可比對 fingerprint：

```powershell
.\target\debug\maple-agent-market.exe assets verify team-pack --expect <64位SHA-256>
```

啟動時使用實際 pack 目錄：

```powershell
.\target\debug\maple-agent-market.exe floating --pack-dir C:\path\to\team-pack
```

## GitHub 邊界

本 repo 不提供 NEXON / MapleStory 素材 downloader，也不允許把遊戲圖片、紙娃娃、地圖、怪物、傳點、技能、音樂或其衍生物提交到 Git。朋友之間分享仍是再散布，必須有相應權利。

若新增可公開的第三方素材，PR 必須同時提供：

- 原作者與第一方來源 URL；
- 精確授權檔；
- 取得日期、版本與修改說明；
- `THIRD_PARTY_NOTICES.md` 更新；
- `policy/public-release/media-licences.toml` 對照；
- `policy/public-release/media-allowlist.sha256` 的新 hash。

每次變更 media 後執行 `python scripts/public-release-audit.py`。
