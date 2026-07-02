# Page-Turn Timing Breakdown

## Scope

The diagnostic timing breakdown localizes the non-BUSY portion of an Xteink X4 page turn without changing page-turn semantics, SSD1677 commands, refresh mode selection, queue behavior, ADC input handling, or display output. Existing `DISPLAY_REQUEST_START`, `DISPLAY_REQUEST_END`, `BUSY_WAIT_START`, and `BUSY_WAIT_END` events remain valid. The new data explains what makes up `display_request_ms - busy_wait_ms` so optimization work can target the dominant stage.

The design uses the detailed split requested for optimization: plane writes are separated into decode/fill time and SPI write time. This is preferred over a coarse stage-only split because the current unexplained time can plausibly be either CPU-bound PackBits/plane reconstruction or bus-bound SSD1677 RAM transfer.

## Event model

Add display-subsystem timing events around the existing BW differential render path. Each event is emitted only from instrumentation points and does not alter renderer control flow.

| Event | arg0 | arg1 | arg2 |
|---|---:|---:|---:|
| `PAGE_METADATA_READ` | source page | target page | duration ms |
| `PLANE_WRITE_START` | plane role | RAM target | plane byte length |
| `PLANE_ROW_FILL_SUMMARY` | plane role | fill/decode duration ms | row count |
| `PLANE_SPI_WRITE_SUMMARY` | plane role | SPI write duration ms | bytes written |
| `PLANE_WRITE_END` | plane role | total duration ms | status |
| `REFRESH_TRIGGER` | refresh mode | duration ms | status |

Plane roles are stable integer codes: `0=previous_fast_base`, `1=target_fast_base`. RAM targets follow controller RAM selection: `0=BW/black`, `1=red`. Status uses `0=ok`, `1=error`, `2=cancelled` if a future epoch-aware path reuses the event shape.

## Measured stages

For a normal BW page turn, `render_bw_differential_observed()` reports:

1. `page_metadata_ms`: both `read_x4_page(book, from)` and `read_x4_page(book, target)`.
2. `prev_plane_total_ms`: previous page fast-base write to red RAM.
3. `prev_plane_fill_ms`: cumulative time spent in row fill/decode callbacks for that plane.
4. `prev_plane_spi_ms`: cumulative time spent in SSD1677 row-data writes for that plane.
5. `target_plane_total_ms`: target page fast-base write to BW RAM.
6. `target_plane_fill_ms`: cumulative time spent in row fill/decode callbacks for that plane.
7. `target_plane_spi_ms`: cumulative time spent in SSD1677 row-data writes for that plane.
8. `refresh_trigger_ms`: the partial-refresh command before the controller BUSY wait.
9. Existing `busy_wait_ms`: cumulative observed SSD1677 ready wait inside the display request.

The analyzer keeps `display_request_ms` as the authoritative end-to-end display-task request duration. It then prints `non_busy_ms = display_request_ms - busy_wait_ms` and `unattributed_ms = display_request_ms - busy_wait_ms - page_metadata_ms - prev_plane_total_ms - target_plane_total_ms - refresh_trigger_ms`. Small unattributed values are expected for engine bookkeeping and event emission.

## Implementation boundaries

The timing hooks live at reusable crate boundaries, not in board-specific application logic. `xteink-x4-display` owns render-stage timing events because it owns the page-source, plane-write, and panel-refresh pipeline. `binbook-fw` maps those display events into diagnostic log records. `ssd1677-driver` continues to own only controller-level row writes, refresh triggering, and BUSY waits.

The row fill and SPI timing split is collected inside the existing row streaming loop. The fill timer wraps the `decoder.fill(...)` call and row copy. The SPI timer wraps each `spi.write(&row)` transfer. The implementation emits summaries per plane instead of one record per row to avoid flooding the bounded diagnostic log.

## Analyzer output

`scripts/analyze_timing.py` adds fields to each completed page-turn line:

```text
page_metadata_ms=<n> prev_plane_total_ms=<n> prev_plane_fill_ms=<n> prev_plane_spi_ms=<n> target_plane_total_ms=<n> target_plane_fill_ms=<n> target_plane_spi_ms=<n> refresh_trigger_ms=<n> non_busy_ms=<n> unattributed_ms=<n>
```

The analyzer tolerates older logs by treating missing new events as unavailable rather than fabricating zeros. Complete breakdown rows require the existing page-turn timeline plus the new summary events within the matching `DISPLAY_REQUEST_START..DISPLAY_REQUEST_END` window.

## Busy-wait optimization interpretation

`busy_wait_ms` is controller/panel waveform time after refresh triggering. It is not normally reducible by CPU or SPI optimization. It can change if the firmware changes refresh mode, LUT, panel mode, update region, refresh scheduling, or quality target. For the current partial BW page turn, the safer optimization targets are the non-BUSY buckets first; any attempt to reduce `busy_wait_ms` must be treated as a display-quality and ghosting tradeoff and verified with live panel images.

## Verification

Host tests cover protocol event-name mapping, runtime event-to-log mapping, row fill/SPI summary accounting with fake SPI and deterministic time, and analyzer parsing of old and new logs. Live-device verification flashes diagnostic firmware, captures `diag page next` logs, runs the analyzer, and confirms that the reported breakdown approximately reconciles:

```text
display_request_ms ~= busy_wait_ms + page_metadata_ms + prev_plane_total_ms + target_plane_total_ms + refresh_trigger_ms + unattributed_ms
prev_plane_total_ms ~= prev_plane_fill_ms + prev_plane_spi_ms + small overhead
target_plane_total_ms ~= target_plane_fill_ms + target_plane_spi_ms + small overhead
```

Hardware evidence includes the exact flash command, serial logs, analyzer output, final `diag status`, and webcam capture of the resulting page state.
