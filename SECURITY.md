# 安全性政策

Maple Agent Market 目前是開源原型，僅支援 `main` 最新版本；尚未承諾舊版維護或回覆時限。

## 隱私與資料流

- 程式不含 analytics、telemetry、crash upload 或自動更新請求。
- agent transcript 與 session 只在本機唯讀解析，不會自動上傳。
- `pixtuoid-hook` 只透過本機 Unix socket 或 Windows named pipe 傳遞單筆事件，不開放網路 listener。
- floating 的 sprite pack 與 BGM 都從本機檔案讀取。
- Log 預設寫入 `~/.cache/pixtuoid/log`，crash log 寫入 `~/.cache/pixtuoid/crash.log`。

## 會修改的設定

只有使用者明確執行 `connect`／`disconnect` 時，程式才會編輯對應 agent CLI 的 hook 設定。寫入採鎖定、備份與 atomic replace，且可用相反命令復原。

執行 pull request 前，請確認沒有提交：

- Codex／其他 agent transcript、session 或 hook payload；
- API key、token、cookie、憑證或 `.env`；
- 個人絕對路徑、private pack、cache、BGM 或未核准 binary；
- NEXON／MapleStory 私人評估素材。

可用下列命令檢查 repo 候選版：

```powershell
python scripts\public-release-audit.py --selftest
python scripts\public-release-audit.py
```

## 回報漏洞

請使用本 repo 的 [GitHub private vulnerability reporting](https://github.com/SaM-runtime/maple-agent-market/security/advisories/new)，不要公開建立含 exploitation 細節的 issue。

如果問題也能在未修改的 Pixtuoid 重現，請另行與上游維護者協調；在修補完成前不要公開未修正漏洞。
