//! Generic `SdStorage` — wraps `embedded_sdmmc` to mount, enumerate, and read
//! files from a FAT-formatted SD card over an `embedded_hal::spi::SpiDevice`.

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::filesystem::{LfnBuffer, Mode, TimeSource};
use embedded_sdmmc::{BlockDevice, SdCard, VolumeIdx, VolumeManager};

/// Format an SFN (8.3) `ShortFileName` into `buf` and return a `&str`.
///
/// SFN is stored space-padded: 8 bytes base + 3 bytes extension.
/// The Display output is `BASENAME.EXT` (dot omitted when extension is empty).
/// Max output length is 12 (8 + '.' + 3).
fn format_short_name<'a>(name: &embedded_sdmmc::filesystem::ShortFileName, buf: &'a mut [u8; 13]) -> &'a str {
    let base = name.base_name();
    let ext = name.extension();
    let mut pos = 0;
    if !base.is_empty() {
        buf[pos..pos + base.len()].copy_from_slice(base);
        pos += base.len();
    }
    if !ext.is_empty() {
        buf[pos] = b'.';
        pos += 1;
        buf[pos..pos + ext.len()].copy_from_slice(ext);
        pos += ext.len();
    }
    // SFN bytes are ISO-8859-1; ASCII is valid UTF-8.
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

/// Generic SD + FAT handle.
///
/// `D` is a `BlockDevice` — either an `SdCard<SPI, DELAY>` (hardware) or a
/// mock for host testing. `TIME` is a `TimeSource` for FAT timestamps.
pub struct SdStorage<D, TIME>
where
    D: BlockDevice,
    TIME: TimeSource,
{
    volume_mgr: VolumeManager<D, TIME>,
}

/// Errors from `SdStorage` operations.
#[derive(Debug)]
pub enum StorageError<D: BlockDevice> {
    /// The underlying block device or filesystem returned an error.
    Sdmmc(embedded_sdmmc::Error<D::Error>),
    /// The named file was not found.
    NotFound,
}

// --- Hardware path: construct from SPI device + delay ---

impl<SPI: SpiDevice<u8>, DELAY: DelayNs, TIME: TimeSource>
    SdStorage<SdCard<SPI, DELAY>, TIME>
{
    /// Construct from an SPI device + delay (real hardware path).
    pub fn new(spi: SPI, delay: DELAY, time: TIME) -> Self {
        let sdcard = SdCard::new(spi, delay);
        let volume_mgr = VolumeManager::new(sdcard, time);
        Self { volume_mgr }
    }
}

// --- Host-test path: construct directly over a BlockDevice ---

impl<D: BlockDevice, TIME: TimeSource> SdStorage<D, TIME> {
    /// Construct from a raw `BlockDevice` (host-test path, no real SPI/delay).
    pub fn from_block_device(block_device: D, time: TIME) -> Self {
        let volume_mgr = VolumeManager::new(block_device, time);
        Self { volume_mgr }
    }

    /// Enumerate files in the root directory, calling `visit` for each.
    pub fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), StorageError<D>> {
        let volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(StorageError::Sdmmc)?;
        let dir = volume
            .open_root_dir()
            .map_err(StorageError::Sdmmc)?;

        let mut lfn_storage = [0u8; 256];
        let mut lfn_buf = LfnBuffer::new(&mut lfn_storage);
        let mut sfn_buf = [0u8; 13];

        dir.iterate_dir_lfn(&mut lfn_buf, |entry, lfn| {
            let name = match lfn {
                Some(n) if !n.is_empty() => n,
                _ => format_short_name(&entry.name, &mut sfn_buf),
            };
            if !name.is_empty() {
                visit(name, entry.size as u64);
            }
        })
        .map_err(StorageError::Sdmmc)?;

        Ok(())
    }

    /// Read `out.len()` bytes at `offset` from file `name`.
    pub fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>> {
        let volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(StorageError::Sdmmc)?;
        let dir = volume
            .open_root_dir()
            .map_err(StorageError::Sdmmc)?;

        let file = dir
            .open_file_in_dir(name, Mode::ReadOnly)
            .map_err(|_| StorageError::NotFound)?;

        file.seek_from_start(offset as u32)
            .map_err(StorageError::Sdmmc)?;

        let n_read = file.read(out).map_err(StorageError::Sdmmc)?;

        if n_read < out.len() {
            for b in out.iter_mut().skip(n_read) {
                *b = 0;
            }
        }

        Ok(())
    }

    /// Return the byte length of `name`, or `StorageError::NotFound`.
    pub fn file_size(&mut self, name: &str) -> Result<u64, StorageError<D>> {
        let volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(StorageError::Sdmmc)?;
        let dir = volume
            .open_root_dir()
            .map_err(StorageError::Sdmmc)?;

        let file = dir
            .open_file_in_dir(name, Mode::ReadOnly)
            .map_err(|_| StorageError::NotFound)?;

        Ok(file.length() as u64)
    }
}
