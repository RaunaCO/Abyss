---
tags: [tide, incident]
---
# Window Drag Crash (2026-11-06)

**Symptom:** moving the window by the title bar closed TIDE (Windows event:
`0xc0000409` in `ucrtbase.dll`, i.e. a Rust panic turned into an abort).

**Cause:** `winctl::start_drag` *sent* `WM_NCLBUTTONDOWN/HTCAPTION`. Windows'
move loop ran inside gpui's mouse handler while `AppView` was borrowed; gpui's
own tasks (the 200 ms ticker) ran inside that loop and panicked with
"RefCell already borrowed".

**Fix:** post the message instead, so the move loop starts from the message
pump. Reproduced and verified with a test copy moved 60 times
(`SetWindowPos`).

**Also added:** a panic hook writing `%APPDATA%\tide\crash.log` with a
backtrace, since panics in window callbacks abort without output.

Related: [[Testing Workflow]].
