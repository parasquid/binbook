# X4 Navigation Burst Diagnostics

## Scope

The diagnostic localizes rapid mixed-direction navigation stalls without changing ADC thresholds, polling cadence, cooldown, queue capacity, page-turn semantics, display timing, SSD1677 commands, or controller-RAM behavior. Diagnostic protocol version 1 and the STATUS payload layout remain unchanged.

## Fixture and exercise

`nav_probe.binbook` contains 16 numbered `GRAY2_PACKED` pages. Every page retains the orientation border, corner and edge labels, crosshair, rulers, asymmetric marker, faint grid, and four grayscale swatches. Pages add distinct checker, band, diagonal, crosshatch, rectangle, bar, quadrant, dot, and X patterns.

Each interior round goes to page 8, clears logs, sends 16 protocol-v1 KEY frames through the bounded FIFO, queries STATUS, and paginates logs. Ten rounds exercise 160 keys. A final five-key boundary sequence from page 0 expects pages `0,1,0,0,1` and exactly two clamped no-op events.

## Structured evidence

| Event | arg0 | arg1 | arg2 |
|---|---|---|---|
| `INPUT_TRANSITION` | GPIO1 ADC | GPIO2 ADC | button, or `-1` |
| `INPUT_DECISION` | button, or `-1` | `0` press, `1` release, `2` cooldown | elapsed ms |
| `TURN_STARTED` | protocol sequence, or `-1` | source page | target page |
| `TURN_BOUNDARY_NOOP` | protocol sequence, or `-1` | current page | page-turn code |

The CLI writes JSON Lines records for run start, keys, STATUS, firmware logs, round results, and the final result. Response order and elapsed time are recorded at arrival. Every moving key requires a sequence-matched start and completion, and STATUS must independently match the expected clamped page.

## Camera correlation and localization

The runner records `/dev/video1`, runs the single serial-owning CLI process, and extracts `crop=440:770:770:250` at each key timestamp. Contact sheets label sequence and expected page. Correct completion and STATUS with a wrong visible page localizes the SSD1677 RAM/activation path. Clean serial bursts leave physical ADC/debounce as the evidence-gated follow-up.

## Accepted KEY request contract

Every accepted directional protocol KEY press enters the same bounded FIFO as a typed relative `PageTurn`, including a turn that is a no-op at a page boundary. FIFO dequeue order is the queue-tail logical page model: each turn resolves against the outcome of every earlier accepted turn, not against the committed page observed while parsing the request. A boundary turn therefore reaches the display engine, emits sequence-matched `TURN_BOUNDARY_NOOP` and completion evidence, and performs no display operation.

This contract preserves the physical button mapping because protocol KEY and ADC input still produce the same `PageTurn` variants. It adds no queue or framebuffer storage, does not change the 16-request capacity, and retains bounded RAM usage.
