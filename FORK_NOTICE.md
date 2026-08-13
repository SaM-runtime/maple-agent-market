# Fork 與非官方聲明

Maple Agent Market 是以 [Pixtuoid](https://github.com/IvanWng97/pixtuoid) 為基礎開發的非官方 agents orchestration visualizer。

Fork 基準為 Pixtuoid `v0.16.0` commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)。本 repo 保留 Pixtuoid 原作者 Ivan Wang 的 MIT 版權與授權聲明；Maple Agent Market 貢獻者的聲明只涵蓋後續修改。

## 保留與移除的上游範圍

保留的是目前 floating runtime 仍直接使用的 source / hook、event decoder、reducer、場景模擬、路徑、RGB buffer、sprite pack 格式與相容識別碼，以及少量 parser 測試 fixture。

已移除終端 TUI、舊辦公室 sprite、robot / skeleton 範例 pack、`run`、`init-pack`、`public-classic` 安裝器、completion / man-page 產生器與失效的本機 skin workshop。Pixtuoid 預設辦公室圖像不再隨本 repo 或 binary 提供。

## 不屬於 MIT repo 的本機素材

下列內容不會隨 source、release、binary 或文件截圖發布：

- NEXON ranking / Open API 紙娃娃及生成的 `.sprite`；
- 自由市場、弓箭手村等遊戲截圖、重建或 WZ / client 衍生背景；
- NEXON / MapleStory 商店框、角色、怪物、傳送點、動作與技能影格；
- MapleStory 音樂、YouTube 下載內容與含上述素材的 preview / QA 截圖；
- 使用者 skin、cache、backup 與 active pack。

使用者可在自己電腦以 `--pack-dir` 載入有權使用的素材；載入行為不會把素材改成 MIT，也不代表本專案取得再散布權。

## 非官方聲明

本專案未受 NEXON、遊戲橘子、OpenAI、Pixtuoid 維護者或其他第三方贊助、認可、核准或背書。MapleStory、NEXON 與相關名稱、角色及標誌屬各自權利人。

本機 BGM 功能只讀使用者指定的 MP3、WAV、OGG 或 FLAC；專案不含 downloader、音訊擷取器或隱藏 web player。
