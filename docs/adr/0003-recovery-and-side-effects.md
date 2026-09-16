# ADR 0003 — Persistent headless runs without fabricated recovery

Status: partial implementation.

A detached per-task supervisor owns execution after the launcher exits. SQLite holds
heartbeats, state and atomic dispatch keys. Linux reconciliation uses PID birth identity.
There is no universal host/process restoration, PTY resume, or exactly-once external write.

Cancellation handles the original process group and does not undo external effects.
An ambiguous launch remains observable rather than blindly retried. Full durable dispatch,
remote fencing and external operation journals are future gates. The initial worker only
performs approved local tasks; push/merge/deploy APIs are deliberately absent.
