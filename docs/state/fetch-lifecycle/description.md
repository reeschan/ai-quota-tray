# プロバイダ取得

`spawn_scheduler` がプロバイダごとに `LoopState` を持つ。`due` のとき `Provider::fetch` する。データがある成功は `succeed` で次の `refresh_secs` まで待つ。`error` があり `has_data` でない失敗は `fail` で、`backoff_delay` が 30 秒から倍々になり `refresh_secs` で頭打ちになる。ファイル監視の `kick` は直前の取得から 2 秒空いていればすぐ `due` にする。
