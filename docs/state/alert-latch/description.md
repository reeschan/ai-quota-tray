# 通知ラッチ

`crossing_alerts` は、使用率が `notify_used_pct` 以上、または同じ通貨の残高が `notify_balance_below` 未満のときだけ `Alert` を作る。キーは `Shared.latches` と SQLite に残り、回復するまで再通知しない。今回のスナップショットが指標を報告しないときは、そのラッチを消さない。`notify_alerts` が Windows トーストを出す。
