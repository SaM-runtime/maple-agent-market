# 公開發布邊界

## 可以發布

- Maple Agent Market 自有 MIT 程式碼、程序化像素 UI 與程序化特效；
- 依 MIT 保留的 Pixtuoid 核心程式與 parser 測試 fixture；
- Monaspace Neon OFL 字型；
- 本機素材 downloader／converter 程式碼、來源 ID／URL 配方與明確同意流程；
- 不含私人媒體的 source、release binary、README snapshot 與 CI artifact。

程式化畫面必須是本專案自行設計的泛用 2D 表現，不能以取樣、描摹或轉檔方式重製受保護遊戲影格。

## 不可以隨本 repo 發布

- NEXON / MapleStory 角色、怪物、地圖、商店框、傳送點、技能與 UI 圖像；
- 遊戲截圖、影片 frame、WZ / client 拆包或重建內容；
- ranking / Open API 紙娃娃與它們的 `.png` / `.sprite` 衍生檔；
- MapleStory BGM、YouTube 擷取或轉存音訊；
- 使用者 private pack、skin、session、transcript、cache、backup 與 QA 擷取。

這個界線同時適用 source tree、Git 歷史、release archive、binary 內嵌資源、README / issue 圖片與 CI artifact。private GitHub repo、限朋友 clone、免費或非營利，都不是再散布授權。

NEXON 官方 Game IP 指引把角色、怪物、圖片、背景音樂與影片列為 Game IP，並未提供可把 client / API 衍生素材併入 MIT GitHub repository 的開源授權。參考：[NEXON Korea Game IP Guide](https://m.nexon.com/terms/716)、[NEXON America Game IP Guide](https://maplestoryworlds.nexon.com/en/legal/policy/1037)。發布前若取得涵蓋修改、GitHub source、release binary、再散布與再授權的書面許可，才可另案重新評估。這份文件是工程政策，不是法律意見。

## 發布 gate

```powershell
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
python scripts\stage-public-release.py --selftest
python scripts\build-public-release.py --selftest
python scripts\build-public-release.py
```

`public-release-audit.py` 會 fail closed 檢查未核准 media、private path、音訊、封裝檔、憑證樣式與本機絕對路徑。allowlist 只對精確 hash 有效；檔案內容改變後必須重新做來源與視覺審查。

release bundle 必須同時包含 `maple-agent-market(.exe)` 與 `pixtuoid-hook(.exe)`；manifest 保留相容用的 `entrypoint` 指向主程式，並以 `entrypoints` 列出兩者，同時明示 `contains_private_maple_assets: false`。前者開啟浮動視窗，後者供 Codex 等來源整合傳遞事件，缺一就不是可用的完整發行包。

bundle 也會帶入 `tools` 內的本機 starter 產生器、Maple Atelier 匯入器、工作坊、BB 前技能同步器與 recipe JSON，manifest 以 `includes_local_asset_bootstrap_tools: true` 記錄。這些是程式碼與來源座標，不是 NEXON 圖像；執行後生成的 `private-assets` 仍不得進入公開 archive。

## clone 驗收

完成 push 後，應在全新目錄從 GitHub remote clone 指定 commit，執行 locked workspace binary build，確認兩個執行檔都存在，再完成兩條 smoke：(1) 不提供 `--pack-dir` 的程序化 fallback 與 hook 定位；(2) `-StarterOnly` 在隔離暫存路徑建立並通過 `validate-pack`。網路配方另以至少一個公開 outfit 做即時 API 驗證，但不能把該測試輸出放入 repo 或 CI artifact。只有這些路徑成功，才能聲稱該 revision 可由乾淨 clone 使用。
