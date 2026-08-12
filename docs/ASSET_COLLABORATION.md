# 素材共同開發

Maple Agent Market 用兩個值定義「大家完全相同」：

1. 相同的 Git commit SHA；
2. 相同的素材 pack SHA-256 fingerprint。

只要兩者都一致，程式碼與被載入的公開素材就是同一份。螢幕縮放、字型 fallback、作業系統與當下 agent events 仍可能造成畫面呈現差異，這些不屬於素材漂移。

## 安裝相同的公開素材

`public-classic` 是編進目前 binary 的可再散布素材，不會從 NEXON、遊戲 CDN、Open API、WZ/client 或 YouTube 下載。它的來源與 repo 內 renderer 使用的 Pixtuoid MIT fallback 完全相同。

```powershell
git rev-parse HEAD
cargo build --locked -p pixtuoid

$installed = .\target\debug\pixtuoid.exe assets install public-classic --json | ConvertFrom-Json
$installed

.\target\debug\pixtuoid.exe assets verify public-classic `
  --expect $installed.fingerprint_sha256

.\target\debug\pixtuoid.exe --theme maple floating `
  --pack-dir $installed.path
```

預設安裝位置為：

- Windows：`%USERPROFILE%\.config\pixtuoid\packs\public-classic`
- macOS／Linux：`~/.config/pixtuoid/packs/public-classic`

可用 `assets --root <PATH>` 指定另一個隔離目錄。`--force` 只會替換帶有相同 managed manifest 的 pack；不會刪除同名但不受 Maple Agent Market 管理的普通資料夾。

## 比對團隊素材

由一位共同開發者提供 fingerprint，其他人用 `--expect` 驗證：

```powershell
.\target\debug\pixtuoid.exe assets list --json
.\target\debug\pixtuoid.exe assets verify public-classic `
  --expect <團隊提供的64位SHA-256>
```

只傳 fingerprint 不會傳送任何素材內容；它只能證明本機檔案是否逐位元一致。

## 匯入團隊有權使用的自訂 pack

如果團隊已經擁有一個可使用的 sprite pack：

```powershell
$team = .\target\debug\pixtuoid.exe assets import C:\team\authorized-pack `
  --id team-pack --json | ConvertFrom-Json

.\target\debug\pixtuoid.exe assets verify team-pack `
  --expect $team.fingerprint_sha256
```

匯入器只複製 `pack.toml` 與 `[animations.*].frames` 實際引用的檔案；不會複製同一資料夾內的其他文件，也不會上傳內容。產生的 manifest 一律標成 `local-only`，因為「能匯入」不等於「具有再散布授權」。

若素材確實由團隊原創或具有 MIT／CC0 等明確再散布授權，可經授權審查後加入 repository 與公開 media inventory。不要把 `local-only` pack、遊戲截圖、紙娃娃、怪物、傳點、技能影格或 BGM 直接 commit。

## 共同修改流程

公開素材的權威來源仍是 Git repository，而不是某位成員的 managed install：

1. 從相同 `main` 建立 feature branch；
2. 修改有授權依據的 `.sprite`／程式化場景；
3. 執行測試與 `public-release-audit.py`；
4. commit、push、由另一位成員 review；
5. 所有人 pull 同一 commit，重新 build；
6. `assets install public-classic --force`；
7. 用 `assets verify --expect ...` 比對新 fingerprint。

這樣素材與程式碼會沿 Git 歷史同步，任一成員都能看出哪個 commit 改了哪些像素。

## 不提供的下載來源

素材管理器不會實作以下來源：

- NEXON／MapleStory CDN 或 Open API 圖片抓取；
- WZ 或遊戲 client 拆取；
- YouTube 音訊下載或轉檔；
- 未附作者、來源、授權與固定 hash 的第三方壓縮包。

GitHub public/private、免費或只邀請朋友，都不會自動產生這些素材的複製與再散布權。若日後取得涵蓋 repository、release binary、修改與再散布的書面許可，才可把對應 pack 納入公開 catalog。
