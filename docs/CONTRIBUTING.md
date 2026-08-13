# 共同開發

## 開始

```powershell
git clone https://github.com/SaM-runtime/maple-agent-market.git C:\dev\maple-agent-market
Set-Location C:\dev\maple-agent-market
git switch -c feature/my-change
cargo build --locked --workspace --bins
.\target\debug\maple-agent-market.exe
```

公開版的基本畫面是程序化 renderer，不需要取得素材包。

## 修改原則

- 新產品功能以 floating window 與 Maple world 為入口；不要恢復已移除的終端 TUI 或辦公室資產。
- 內部 `pixtuoid-*` 名稱是上游 hook / wire / config 相容層。更名必須帶 migration、回滾與跨來源測試。
- source decoder 不猜測資料；沒有可靠 `parent_id` 就不建立 parent-child 關係。
- pack animation 是 optional override；缺少時必須保持程序化 fallback 可用。
- 不要提交 private pack、session、cache、絕對路徑、憑證或沒有再散布權的 media。

## 驗證

最小檢查：

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace
python scripts\public-release-audit.py
```

提交或 PR 前的完整 gate：

```powershell
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
python scripts\public-release-audit.py --selftest
python scripts\build-public-release.py --selftest
python scripts\stage-public-release.py --selftest
```

安裝了 `just` 時可執行 `just preflight`。UI 變更另需重新產生 snapshot 並目視確認：

```powershell
just snapshot
```

## 公開 bundle

公開 bundle 必須同時包含 `maple-agent-market(.exe)` 與 `pixtuoid-hook(.exe)`。前者是浮動視窗，後者是來源整合使用的 hook shim；不要只發行其中一個。

```powershell
# 以公開路徑重寫建置兩個執行檔，並對兩者進行 binary 稽核。
python scripts\build-public-release.py

# 僅接受乾淨、已 commit 的 revision；輸出必須在 repository 外且不存在。
python scripts\stage-public-release.py --output C:\dist\maple-agent-market-0.16.0
```

輸出的 `PUBLIC_BUNDLE_MANIFEST.json` 必須保留 `entrypoint`（主程式），並以 `entrypoints` 列出兩個執行檔；`SHA256SUMS.txt` 必須涵蓋其自身以外的每個 bundle 檔案。安裝了 `just` 時，對應指令為 `just release-build` 與 `just release-stage C:\dist\maple-agent-market-0.16.0`。

## 素材 PR

任何新增媒體都必須有第一方來源、明確再散布授權、NOTICE、授權檔與精確 hash。NEXON / MapleStory 圖像、音樂、截圖、WZ / client、Open API 紙娃娃及其衍生物不接受進入 public repo。

## 提交範圍

不要用 broad cleanup 混入無關變更。若刪除上游程式，PR 應附上目前 runtime 的引用追查與對應測試，證明被刪除內容不在 source → reducer → scene → floating 呼叫鏈上。
