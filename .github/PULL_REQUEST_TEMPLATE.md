## 變更內容

請簡述這次修改解決的問題，以及對 floating 視窗或 agent 狀態的影響。

## 驗證

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --locked --workspace`
- [ ] `python scripts/build-public-release.py --selftest`
- [ ] `python scripts/stage-public-release.py --selftest`
- [ ] `python scripts/public-release-audit.py`
- [ ] 若修改畫面，已用公開安全素材目視確認

## 素材確認

- [ ] 沒有提交 NEXON / MapleStory 遊戲素材、音樂、截圖或其衍生檔
- [ ] 新增的第三方素材具有明確再散布授權，並已更新 NOTICE 與 hash inventory
- [ ] 沒有提交本機 session、cache、private pack、絕對路徑或秘密資訊
