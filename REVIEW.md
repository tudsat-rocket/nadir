# Review findings

- **Downlink alarm goes quiet on total link loss** (`nadir/src/widgets/alerts.rs:82-93, 146`,
  `shell/status_bar/vitals.rs:205-218`). With no frames in the 1 s rate window, downlink quality is
  `None`, which the `< LINK_ALARM_QUALITY` filter never alarms on. Uplink keeps the last
  `LINK_NODE_STATUS` forever, typically green. Fix: previously-heard system with zero rate reads
  `Some(0.0)` / "DOWNLINK LOST"; uplink `None` once `LINK_NODE_STATUS` is a few seconds old.
- **Param download fails permanently on lossy links** (`nadir-core/src/protocols.rs:97, 148-167`,
  `protocols/params.rs:482-505`). Discovery ends at the first 1 s gap; re-requests discard any
  `PARAM_VALUE` whose index is not the one just requested; one index missing after 3 retries drops
  everything received; `Failed` is never retried. Needed before params-via-LoRa lands. Fix: accept
  any missing index in phase 2, keep partial results, retry after failure.
- **Corrupted frames create phantom systems** (`nadir-core/src/source.rs:338-352`). The `System` is
  inserted before any decode (CRC is checked in `decode`), so noise yields random system ids with
  permanent protocol tasks (heartbeats, `SET_MESSAGE_INTERVAL` every ~1.5 s, param/mode requests),
  GUI entries and tlogs. Irrelevant over UDP / the LoRa bridge, bites on noisy serial. Fix: create
  the system only after a successful decode.
- **CAN proxy blocks a tokio worker when `vcan0` is missing** (`nadir-core/src/lib.rs:136-138`,
  `can_proxy.rs:16-51`). Synchronous `std::thread::sleep(3s)` retry loop inside `rt.spawn`, i.e. on
  every Linux machine without `vcan0`; starves links on 1-2 core machines. Also: socket opened twice
  (`unreachable!()` if the second fails), reader spins without backoff on persistent errors,
  interface name hardcoded. Fix: async open loop with `tokio::time::sleep`, reader backoff/reopen.
- **UDP server dies silently on any receive error** (`nadir-core/src/links/udp.rs:12-37`, maviola
  0.3.0 `asnc/io/transport/udp/server.rs:44`). maviola's `recv_from(..)?` ends the connection,
  `listen` returns `Ok` without logging, never restarted. Rare on Linux; on Windows an ICMP
  port-unreachable (peer rebooted while we send heartbeats) surfaces as `WSAECONNRESET`. Receive
  buffer is 512 bytes, so datagrams packing several frames are truncated. Fix: retry loop with
  backoff around `listen` (as in `tcp::run`); buffer needs a maviola patch.
- **Unbounded memory and per-frame cost.**
  - Store never evicts; systems/tasks never removed; Vec regrowth happens under the `Db` mutex
    (`nadir-store/src/lib.rs` ~277-301).
  - GUI event `std::mpsc` (`nadir/src/app.rs:49, 71`) clones every frame and drains only in `ui()`,
    which eframe skips while minimized / backgrounded on Android. Grows unbounded, replays all
    ack toasts on restore. Fix: forward only ACK and peer events, or drain in `logic()`.
  - Every typed query (`last_message` etc., `slices` ~300-318) scans the whole `SeriesMap` under
    the lock; the rocket pane alone does ~30 per frame, repaint happens per received frame. Fix:
    O(1) index by `(sys, comp, msg_id, instance)`.
  - Plot windows of 2k-32k samples are rescanned uncached per repaint; a new cache level scans the
    series from sample 0 (`line` 674-694). Commands pane clones and sorts full history per frame
    (`commands.rs:304`).
- **Timestamp ordering.** Live frames are stamped `Utc::now()` (`nadir-core/src/lib.rs:191`), tlog
  replay uses file stamps unchecked, but `window_range` (`nadir-store/src/lib.rs:327-338`) assumes
  sorted series. A backwards clock step (NTP resync) breaks plot windows, `messages_since` cursors
  and frequency stats. `messages_since` (`:412-446`) is exclusive of the cursor timestamp, so
  same-stamp frames split across polls or `POLL_LIMIT` are lost (CAN pane, map track). Fix:
  monotonic stamping or clamp to `max(t, last_t)` in `push`; index-based cursor.
- **tlog / settings durability.** tlogs flush each second but never `sync_data`, so power loss
  drops the kernel writeback window (the final seconds). One write error sets the system's file to
  `None` for the session (`nadir-core/src/tlog.rs:147-151`). Settings use a plain `fs::write`
  (`settings.rs:147`); a crash mid-save leaves a truncated file that loads as defaults. Fix:
  `sync_data` after the periodic flush, reopen after errors, write-then-rename for settings.
- **Sent commands are stored as vehicle traffic** (`nadir-core/src/system.rs:266-269`). Outgoing
  `COMMAND_LONG`/`COMMAND_INT` are written under (vehicle, component 1), so they appear in message
  counts, rates and plots as if received, and the Commands pane history disagrees with the tlog.
  Fix: store under the ground station's own id, or a per-`System` sent-command list.

## Low urgency

- **USB serial link can stall permanently** (maviola 0.3.0 `asnc/io/transport/serial/serial_rw.rs`,
  used by `nadir-core/src/links/usb.rs`). Reader and writer share a `std::sync::Mutex` via
  `try_lock`; the loser returns `Pending` without registering a waker and is never polled again.
  The reader also busy-spins on `WouldBlock`. Reproduced with a pty pair (2 of 8 runs stalled).
  Fix: own transport on `tokio-serial` with split halves, or patch maviola. Not urgent while
  zenith is reached over ethernet only.
- **Stale telemetry looks live.** Only component rows (`vitals.rs`, `STALE_AFTER`) check age;
  vitals battery/GPS/uptime, sidebar armed/mode/SoC, preflight, rocket schematic valves and
  pressures, map markers and horizon (0 when no data, `map_or(0.0, ..)`) show the last value
  forever. Mostly covered by the link-loss alarm above. Fix: per-system stale overlay after N s
  without frames, then an age-aware `System` accessor for dimming individual readings.
- **Rocket view shows -1% battery** (`nadir/src/panes/propulsion.rs:122, 132-134`,
  `battery_indicator`). zenith sends `battery_remaining = -1` without ADC; sidebar and vitals filter
  it, `battery_indicator` does not. `SYS_STATUS` fallback also shows `UINT16_MAX` V / `-1` A as
  values. Fix: `soc: Option<f32>` via `state_of_charge`, filter `SYS_STATUS` sentinels.
- **Commands pane sends wrong params** (`nadir/src/panes/commands.rs:269-293`). Param7 edits a copy
  (`if let Some(mut v) = self.param7`), so it is always sent as 0.0. Param5/6 are one `u32` read as
  f32 bits in LONG and `i32` in INT, so switching type reinterprets typed values. Fix:
  `&mut self.param7`; separate typed storage for param5/6.
- **Ack feedback is not attributed** (`nadir/src/app.rs:297-318`, `widgets/alerts.rs:136-144`).
  Toasts check neither the sender nor `target_system`/`target_component` and do not name the
  system (rocket and gcs board both ack). The NACK token reflects only the latest ack of any
  command and never expires. A lost command produces no indication. Fix: filter on
  `target_system`, name the sender; eventually pending-command tracking with timeouts in `System`.
- **`set_param` panics on ids over 16 bytes** (`nadir-core/src/system.rs:378-380`). Ids come from
  `from_utf8_lossy(..).trim_matches('\0')` (`protocols/params.rs:207, 232`); a non-UTF-8 byte
  (3-byte U+FFFD) or junk after an early NUL makes them longer. Not reachable with zenith's ASCII
  names. Fix: keep the raw `[u8; 16]` id and send it back; cut at the first NUL for display.
- **Map track cache mixes live and log data** (`nadir/src/panes/map.rs:46, 160-205`).
  `system_paths` is keyed by system id only, not by `behavior.source`, so switching between live
  and a tlog of the same system merges tracks and re-queries every frame. Fix: key by source or
  clear on source change.
