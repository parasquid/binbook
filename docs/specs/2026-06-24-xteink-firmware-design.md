# BinBook Xteink X4 Firmware — Design Spec

Date: 2026-06-24
Status: In Progress
Target: Xteink X4 (ESP32-C3, SSD1677/GDEQ0426T82, 800×480 e-ink)

---

## 1. Scope

### First Iteration

- Load and display `sample.binbook` from flash
- Button navigation: LEFT/RIGHT and UP/DOWN turn pages
- Portrait orientation (logical 480×800, physical 800×480, 270° clockwise logical-to-physical rotation)
- GRAY1 pixel format only (1-bit, 48KB per page)
- Serial CLI commands: flash firmware, upload binbook, list binbooks, delete binbook
- Minimum latency from button press to display change

### Roadmap (Future Iterations)

- UI shell with file picker
- GRAY2 pixel format (2-bit grayscale)
- SD card storage
- Additional buttons (BACK, SELECT, POWER)
- Power management (deep sleep, wake on POWER)
- Pre-fetch next page during display refresh
- Dirty-region refresh optimization

---

## 2. Architecture

### 2.1 Approach: Bare Metal Main Loop

No RTOS, no async, no heap. Single-threaded main loop with direct hardware access.

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                              │
│  loop { poll input → render page → check serial }           │
├──────────┬──────────┬──────────┬────────────────────────────┤
│ xteink-  │ ssd1677- │ binbook  │ binbook-fw                 │
│ hal      │ driver   │ (existing)│ (binary)                  │
├──────────┴──────────┴──────────┴────────────────────────────┤
│               no_std Rust, direct HW access                 │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Crate Structure

```
binbook/
├── rust/                          # Existing binbook-core crate
│   └── Cargo.toml                 # name = "binbook", no_std, lz4 feature
│
├── firmware/                      # New firmware workspace
│   ├── Cargo.toml                 # Workspace root
│   ├── crates/
│   │   ├── xteink-hal/            # Hardware abstraction (traits)
│   │   │   ├── Cargo.toml         # name = "xteink-hal"
│   │   │   └── src/
│   │   │       ├── lib.rs         # Traits: Spi, Gpio, Adc, Flash
│   │   │       ├── esp32c3.rs     # ESP32-C3 trait impls
│   │   │       └── xteink_x4.rs   # X4 board-specific pin mappings
│   │   │
│   │   ├── ssd1677-driver/        # Display driver
│   │   │   ├── Cargo.toml         # name = "ssd1677-driver"
│   │   │   └── src/
│   │   │       ├── lib.rs         # SSD1677 commands, init, refresh
│   │   │       ├── spi.rs         # SPI command layer
│   │   │       └── gray1.rs       # GRAY1 plane streaming
│   │   │
│   │   └── binbook-fw/            # Firmware binary
│   │       ├── Cargo.toml         # name = "binbook-fw"
│   │       └── src/
│   │           ├── main.rs        # Entry point, main loop
│   │           ├── input.rs       # Button polling, debouncing
│   │           ├── display.rs     # Page render pipeline
│   │           └── serial.rs      # CLI command handler
│   │
│   └── build.rs                   # Optional: size reports
│
└── cli/                           # Rust CLI tool (host)
    ├── Cargo.toml                 # name = "binbook-cli"
    └── src/
        └── main.rs                # flash, upload, list, delete
```

### 2.3 Dependency Rules

- `ssd1677-driver` depends on `xteink-hal` via traits, not concrete impls
- `binbook-fw` wires concrete impls to traits at the top level
- No `#[path]` hacks, no sibling references in library crates
- Each crate compiles independently with `cargo build -p <name>`
- No heap allocation in library crates (no `alloc`, no `std`)

---

## 3. Main Loop

```rust
fn main() -> ! {
    // 1. Init hardware
    let mut spi = Spi::new(SPI2, SPI_FREQ_20MHZ);
    let mut display = Ssd1677::init(&mut spi);
    let mut adc = Adc::new(ADC1);
    let mut uart = Uart::new(UART0);
    let mut flash = Flash::new();
    
    // 2. Open sample.binbook
    let binbook = BinBook::open(flash.reader("sample.binbook"), scratch_buf)
        .expect("failed to open sample.binbook");
    
    // 3. Show first page
    let mut current_page: u32 = 0;
    display_page(&mut flash, &mut display, &binbook, current_page);
    
    // 4. Main loop
    loop {
        let button = input::poll(&mut adc);
        match button {
            Some(ButtonEvent::Press(Button::Right | Button::Down)) => {
                if current_page < binbook.page_count() - 1 {
                    current_page += 1;
                    display_page(&mut flash, &mut display, &binbook, current_page);
                }
            }
            Some(ButtonEvent::Press(Button::Left | Button::Up)) => {
                if current_page > 0 {
                    current_page -= 1;
                    display_page(&mut flash, &mut display, &binbook, current_page);
                }
            }
            _ => {}
        }
        serial::tick(&mut uart, &mut flash, &mut current_page);
    }
}
```

---

## 4. Display Pipeline

### 4.1 Streaming Architecture

No framebuffer. Decompressed rows stream directly to SPI:

```rust
fn display_page(flash: &mut Flash, display: &mut Ssd1677, binbook: &BinBook, page: u32) {
    let info = binbook.page_info(page).expect("invalid page");
    
    // Set RAM window (physical coordinates)
    display.set_window(0, 0, 800, 480);
    
    // Stream rows: decompress → SPI write
    let mut row_buf = [0u8; 60]; // GRAY1: 480/8 = 60 bytes/row
    for y in 0..480 {
        decompress_row(flash, binbook, &info, y, &mut row_buf);
        display.write_row(&row_buf);
    }
    
    // Trigger partial refresh (hardware operation, ~260ms)
    display.refresh(RefreshMode::Partial);
}
```

### 4.2 Rotation

Logical portrait (480×800) → Physical landscape (800×480):

```rust
fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    // 270° clockwise rotation, matching the verified SquidScript Xteink X4 target
    let phys_x = 799 - logical_y;              // 0..799
    let phys_y = logical_x;                    // 0..479
    (phys_x, phys_y)
}
```

Stored GRAY1 data is row-major logical portrait. Firmware maps logical rows to physical RAM addresses during SPI transfer.

### 4.3 GRAY1 Pixel Format

- 1-bit per pixel, 8 pixels per byte
- MSB-first: bit 7 = leftmost pixel
- Canonical: 0 = black, 1 = white
- Row size: 480 / 8 = 60 bytes (no padding)
- Page size: 60 × 800 = 48,000 bytes

### 4.4 Decompression

RLE PackBits (BinBook variant):

```
Control byte 0..127: literal run, copy (control + 1) bytes
Control byte 128..255: repeat run, repeat (control & 0x7F) + 1 times next byte
```

Note: 0x80 = repeat 1 byte (not no-op like standard PackBits).

---

## 5. Input Handling

### 5.1 ADC Ladder Buttons

| Button | GPIO | ADC Range | Action |
|--------|------|-----------|--------|
| LEFT | GPIO1 | > 750–1600 | Page prev |
| RIGHT | GPIO1 | ≤ 750 | Page next |
| UP | GPIO2 | > 750–2200 | Page prev |
| DOWN | GPIO2 | ≤ 750 | Page next |
| BACK | GPIO1 | > 2200–2500 | Roadmap |
| SELECT | GPIO1 | > 1600–2200 | Roadmap |
| POWER | GPIO3 | GPIO input | Roadmap |

### 5.2 Debounce Strategy

Edge detection + cooldown (not 30ms blocking):

```rust
struct InputState {
    last_raw: u16,
    last_button: Option<Button>,
    last_press_time: Instant,
    cooldown_ms: u32,  // 100ms between page turns
}

fn poll(state: &mut InputState, adc: &mut Adc) -> Option<ButtonEvent> {
    let raw = adc.read();
    let current = decode_adc(raw);
    
    if current != state.last_button {
        if now.duration_since(state.last_press_time).as_millis() > state.cooldown_ms {
            state.last_press_time = now;
            state.last_button = current;
            return current.map(ButtonEvent::Press);
        }
    }
    
    state.last_button = current;
    None
}
```

---

## 6. Serial Protocol

### 6.1 Transport

USB CDC-ACM (virtual serial port over USB). Baud rate: 115200 (ignored by CDC-ACM, but kept for compatibility).

### 6.2 Commands

```
CLI → Device:
  LIST                    → list .binbook files
  UPLOAD <name> <size>    → start upload (followed by raw bytes)
  DELETE <name>           → delete file
  INFO                    → show firmware version, flash usage
  PAGE                    → show current page number

Device → CLI:
  OK                      → command accepted
  OK <list>               → list response (comma-separated names)
  OK <info>               → info response (JSON-like)
  ERROR <message>         → error
  READY                   → ready for upload data
```

### 6.3 Non-Blocking Processing

Serial commands are processed between page turns via `serial::tick()`. During a 260ms display refresh, USB hardware buffers incoming data. After refresh, we process pending commands.

---

## 7. Memory Layout

### 7.1 SRAM (400KB total)

```
Stack:                    8KB   (main + interrupt)
BinBook scratch buffer: 256B   (header + section table)
Page index entry:       128B   (one at a time)
Compressed page buffer: ~16KB  (max compressed GRAY1 page)
Row buffer:              60B   (GRAY1 row: 480/8)
Serial RX buffer:       256B
Static globals:         ~1KB
─────────────────────────────
Free:                   ~375KB (available for future use)
```

### 7.2 Flash (16MB)

```
Firmware (slot 0):     7.9MB  @ 0x20000
OTA (slot 1):          7.9MB  @ 0x7E0000
Storage:              192KB   @ 0xFB0000
  └── sample.binbook (up to ~180KB)
Coredump:               4KB  @ 0xFFF000
```

### 7.3 No Heap

All buffers are statically allocated. No `alloc` dependency in firmware crates. Deterministic memory usage, no fragmentation.

---

## 8. Latency Budget

| Phase | Duration | Notes |
|-------|----------|-------|
| ADC poll | ~0.1ms | Single-shot read |
| Button decode | ~0.01ms | Range comparison |
| Page index read | ~0.5ms | 128 bytes from flash |
| Compressed data read | ~2-5ms | ~5-15KB RLE GRAY1 |
| Decompress | ~1-3ms | RLE unpackbits |
| SPI transfer | ~19ms | 48KB at 20MHz |
| Display refresh | ~260ms | SSD1677 partial update |
| **Total** | **~285ms** | Button press → pixels on screen |

### Optimization Levers (Future)

1. **SPI clock**: 4MHz → 20MHz (already planned)
2. **Dirty region**: Refresh content area only, not full panel
3. **Pre-fetch**: Decompress next page while current displays
4. **LUT tuning**: Custom waveform table for faster partial refresh

---

## 9. Roadmap

### v1.0 — First Iteration
- [ ] Bare metal firmware: load `sample.binbook`, page turn, serial CLI
- [ ] Rust CLI: flash, upload, list, delete
- [ ] GRAY1 only, portrait only, flash storage only

### v1.1 — GRAY2 Support
- [ ] Add GRAY2 pixel format (2-bit, 96KB per page)
- [ ] GRAY2 plane decomposition (MSB + LSB + BW)
- [ ] Grayscale refresh mode

### v1.2 — UI Shell
- [ ] File picker (list books, select to read)
- [ ] BACK button: return to picker
- [ ] SELECT button: confirm selection
- [ ] Chapter navigation

### v1.3 — Storage Expansion
- [ ] SD card support (FAT32)
- [ ] Move books between flash and SD

### v1.4 — Power Management
- [ ] Deep sleep on POWER long-press
- [ ] Wake on POWER button
- [ ] Battery level display

### v1.5 — Performance
- [ ] Pre-fetch next page
- [ ] Dirty-region refresh
- [ ] Custom LUT for faster partial refresh

---

## 10. SquidScript Reuse

### Crates SquidScript Can Import

| Crate | What SquidScript Replaces |
|-------|---------------------------|
| `binbook` (existing) | C BinBook reader in `vm_runtime_binbook.c` |
| `ssd1677-driver` | C display driver in `ssd1677_gdeq0426t82_display.c` |
| `xteink-hal` (partially) | Zephyr HAL calls (but Zephyr owns GPIO/SPI) |

### What Stays Different

- `binbook-fw` is too specific (board-specific main loop)
- SquidScript uses Zephyr HAL, we use `embedded-hal` traits
- SquidScript has VM/runtime layer, we don't

---

## 11. Open Questions

1. **Flash filesystem**: Raw partition with file table, or LittleFS? SquidScript uses LittleFS. Raw is simpler but less flexible.
2. **CLI binary name**: `binbook-cli` or `binbook` (extending Python CLI)?
3. **Upload flow**: Raw binary transfer, or chunked with CRC verification?

These can be resolved during implementation.
