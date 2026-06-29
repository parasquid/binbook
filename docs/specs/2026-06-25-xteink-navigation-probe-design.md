# Xteink X4 Navigation Probe Design

The embedded navigation probe is a 16-page `xteink-x4-portrait` BinBook fixture used for firmware navigation and display diagnostics. It is compiled as `GRAY2_PACKED` staged-native output at physical `800x480`, with three 30-chunk planes per page and transition masks for both directions between adjacent pages.

Every page carries the persistent orientation and calibration frame. Large labels `PAGE 00` through `PAGE 15` and distinct dominant patterns make stale, skipped, repeated, rotated, mirrored, clipped, or partially written pages visually distinguishable.

Directional buttons and diagnostic KEY requests share the same bounded page-turn path. `Right` and `Down` request the next page; `Left` and `Up` request the previous page. Navigation clamps at pages 0 and 15. Firmware streams 16-row chunks and does not allocate a framebuffer.
