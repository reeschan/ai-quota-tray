# トレイのウィンドウ

`main` ウィンドウは起動時に隠れ、`reveal`（トレイの左クリックか「開く」）で表示する。`WindowEvent::CloseRequested` は終了せず `prevent_close` して `hide` する。設定タブを開いているあいだ、`snapshots-updated` は `snapshots` を更新するだけで `render` しない。使用量タブに戻したときに描く。
