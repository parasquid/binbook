# X4 Boundary Burst Dispatch/FIFO Fix Plan

## Goal

Make boundary and mixed-direction protocol KEY requests preserve accepted FIFO-relative intent, including sequence-matched no-op completion evidence, without changing physical ADC behavior or display rendering.

## Root cause

Diagnostic dispatch decides boundary no-ops against committed page state before queued requests complete. It drops no-op requests before the display engine. Accepted relative requests retain only direction, so a later request accepted against page 0 can resolve against page 1 when dequeued. Live sequences 283, 285, and 286 had command receipts but no engine evidence; sequence 287 was accepted as `Down` at committed page 0 and executed `1→2`.

## Tasks

- [x] Add failing dispatch tests for `[Up, Down, Up, Up, Down]` accepted from page 0 while the first moving request is pending. Require modeled results `0,1,0,0,1` and one completion per sequence.
- [x] Add failing tests that boundary no-ops traverse the observation/completion path and emit `TURN_BOUNDARY_NOOP` without a display operation.
- [x] Choose one typed request contract: reserve absolute target pages at acceptance, or maintain a queue-tail logical page model. Document why it preserves physical button semantics and bounded RAM.
- [x] Implement the minimal dispatch/request change without altering ADC timing, queue capacity, display coordination, or SSD1677 behavior.
- [x] Run focused protocol, aggregator, engine, transport, and scripted nav-burst tests, then full firmware/CLI/Python matrices and pinned builds.
- [x] Flash the diagnostic image, capture 15 seconds of boot serial, rerun the 10-round synchronized camera diagnostic, independently query STATUS/logs, and require the boundary case to settle visibly on page 1 with two no-op events and five completions.

## Completion gate

Do not call the fix complete until all 165 live KEY requests have sequence-matched engine evidence, STATUS and webcam labels agree with the model, counters remain zero, and `HANDOFF.md` records the new acceptance matrix and artifact paths.
