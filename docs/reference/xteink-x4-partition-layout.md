# Xteink X4 partition layout

BinBook uses the Xteink X4's 16 MiB flash geometry that is compatible with the
CrossPoint X3/X4 OTA layout. The two application slots are 6.25 MiB each, so
the CrossPoint 1.5 RC3 image (about 5.24 MiB) fits where the previous BinBook
2.5 MiB slots did not.

The tracked source of truth is
[`firmware/partitions/xteink-x4.csv`](../../firmware/partitions/xteink-x4.csv):

```csv
# Name,Type,SubType,Offset,Size,Flags
nvs,data,nvs,0x9000,0x5000,
otadata,data,ota,0xe000,0x2000,
app0,app,ota_0,0x10000,0x640000,
app1,app,ota_1,0x650000,0x640000,
binbook,data,littlefs,0xc90000,0x360000,
crash,data,0x40,0xff0000,0x10000,
```

| Region | Offset | Size | End (exclusive) |
| --- | ---: | ---: | ---: |
| NVS | `0x009000` | `0x005000` | `0x00e000` |
| OTA metadata | `0x00e000` | `0x002000` | `0x010000` |
| `app0` | `0x010000` | `0x640000` | `0x650000` |
| `app1` | `0x650000` | `0x640000` | `0xc50000` |
| Reserved alignment gap | `0xc50000` | `0x040000` | `0xc90000` |
| BinBook LittleFS | `0xc90000` | `0x360000` | `0xff0000` |
| Crash/coredump reservation | `0xff0000` | `0x010000` | `0x1000000` |

All offsets and sizes are 4 KiB aligned. The app slots and crash/data
boundaries match CrossPoint's X4 layout; only the data subtype is named
`littlefs` because BinBook mounts that partition with LittleFS. BinBook uses
the first 4 KiB of the reserved crash/coredump range for its crash summary.
The data volume
is therefore smaller than BinBook's former 10.93 MiB experiment, but firmware
images are no longer constrained to 2.5 MiB.

Do not use an OTA-slot-only flasher on an erased device: it requires an existing
partition table and reports `Partition table has no otadata partition` when the
table is absent. A first install must write the bootloader at `0x0`, this table
at `0x8000`, and the application at `0x10000`. CrossPoint's official browser
installer normally performs the equivalent repair/full-install path; its OTA
fast-flash control is only for a device that already has this table.

Reference implementation and installer:

- [CrossPoint firmware installation instructions](https://github.com/crosspoint-reader/crosspoint-reader#install-firmware)
- [CrossPoint web flasher](https://crosspointreader.com/#flash-tools)
- [Espressif partition table format](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-guides/partition-tables.html)
