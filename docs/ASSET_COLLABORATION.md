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

若 pack 含多款紙娃娃，可在 `pack.toml` 提供與 `market_avatar_hires` 影格順序一致的選填名稱：

```toml
[characters]
names = ["角色一", "角色二", "角色三"]
```

未提供名稱時會使用內建名稱或「角色 NN」；角色清單數量仍以實際完整紙娃娃影格數為準，不會因名稱較多而建立不存在的角色。

## 公開工具、本機生成的 Maple Atelier catalog

公開 source 與 binary bundle 都包含建立器、工作坊、匯入器與來源配方；它們是可審查的 MIT 程式碼，不含生成素材。第一次建立 catalog：

```powershell
.\建立本機素材.cmd
# 無網路、只產生 8 款原創 starter：
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File .\tools\Bootstrap-MapleLocalAssets.ps1 -ProjectRoot $PWD -StarterOnly
```

正常模式要求 `-AcceptThirdPartyAssetNotice` 才會發出網路請求，並把結果寫入 `.gitignore` 排除的 `private-assets/skins`。配方固定 19 個 Maple Atelier 公開 outfit ID 與一個 maplestory.io 角色 render URL，加上 8 個原創 starter 後共 28 款；友善啟動器另以 `-IncludeClassicSkills` 重建 GMS v62（BB 前）的兩組技能時間軸。來源服務的內容可被作者更新或刪除，因此配方可重播不等於第三方回應具永久 byte-for-byte 保證。

輔助程式接受 Maple Atelier 公開 outfit、`/simulator?load=`，以及 Maple Atelier「複製」所產生的 `maplestory.io` HTTPS 角色渲染網址。它會在安裝根目錄的私有素材區建立完整站立、走路、爬繩、第二站姿、警戒與坐下影格，並重建本機 catalog pack。由角色面板啟動時，主程式會非同步等待 helper 的明確成功／取消／失敗結果；成功後立即重新載入 catalog 並跳到新角色所在頁，不需重啟。自訂角色可用同一個 helper 依 catalog 索引移除；UI 需要同列兩次確認，必要的八個內建槽位會被拒絕。移除時先把來源搬到 install-local `deleted`，原子更新 catalog 並重建 pack；重建失敗會還原來源與 catalog，成功後主程式立即重載並把較後面的角色選擇向前對齊。為避免把正在使用的團隊／自訂 pack 偷換成本機 catalog，熱匯入與刪除只在目前以明確 `--pack-dir` 載入同一個 install-local `catalog-pack` 時可用。裸 `/simulator` 只引用瀏覽器 localStorage，無法單靠網址重建造型，因此會被拒絕。

匯入器程式與 recipe 屬公開 repo；下載結果、轉換後 PNG／sprite、來源快取與 catalog pack 則屬 install-local 邊界，不是公開 bundle 的預生成資產。NEXON-derived 輸出不得因為是公開 outfit、API 回應或經過轉檔就宣告為 MIT；公開發行仍須排除它們。

## GitHub 邊界

本 repo 可以提供 downloader／converter 的程式碼、來源 ID、URL 模板與配方，但不允許把它們下載出的遊戲圖片、紙娃娃、地圖、怪物、傳點、技能、音樂或衍生 `.sprite` 提交到 Git。朋友之間直接交換生成包仍是再散布，必須有相應權利；建議每位使用者在自己電腦明確同意後重播配方。

若新增可公開的第三方素材，PR 必須同時提供：

- 原作者與第一方來源 URL；
- 精確授權檔；
- 取得日期、版本與修改說明；
- `THIRD_PARTY_NOTICES.md` 更新；
- `policy/public-release/media-licences.toml` 對照；
- `policy/public-release/media-allowlist.sha256` 的新 hash。

每次變更 media 後執行 `python scripts/public-release-audit.py`。
