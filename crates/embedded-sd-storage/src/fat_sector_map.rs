#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorPurpose {
    OutsideVolume,
    Reserved,
    Fat { copy: u8 },
    Directory,
    Journal,
    Payload,
    DataOther,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatGeometryError {
    Signature,
    SectorSize,
    ClusterSize,
    ReservedSectors,
    FatCount,
    FatSize,
    TotalSectors,
    Overflow,
    InvalidRange,
    Capacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatGeometry {
    volume_start: u32,
    volume_end: u32,
    fat_start: u32,
    fat_sectors: u32,
    fat_count: u8,
    data_start: u32,
    sectors_per_cluster: u8,
    root_cluster: u32,
}

impl FatGeometry {
    pub fn from_boot_sector(volume_start: u32, boot: &[u8; 512]) -> Result<Self, FatGeometryError> {
        if u16::from_le_bytes([boot[510], boot[511]]) != 0xaa55 {
            return Err(FatGeometryError::Signature);
        }
        if u16::from_le_bytes([boot[11], boot[12]]) != 512 {
            return Err(FatGeometryError::SectorSize);
        }
        let sectors_per_cluster = boot[13];
        if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
            return Err(FatGeometryError::ClusterSize);
        }
        let reserved = u32::from(u16::from_le_bytes([boot[14], boot[15]]));
        if reserved == 0 {
            return Err(FatGeometryError::ReservedSectors);
        }
        let fat_count = boot[16];
        if fat_count == 0 {
            return Err(FatGeometryError::FatCount);
        }
        let root_entries = u32::from(u16::from_le_bytes([boot[17], boot[18]]));
        let total16 = u32::from(u16::from_le_bytes([boot[19], boot[20]]));
        let total32 = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]);
        let total_sectors = if total16 != 0 { total16 } else { total32 };
        if total_sectors == 0 {
            return Err(FatGeometryError::TotalSectors);
        }
        let fat16 = u32::from(u16::from_le_bytes([boot[22], boot[23]]));
        let fat32 = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);
        let fat_sectors = if fat16 != 0 { fat16 } else { fat32 };
        if fat_sectors == 0 {
            return Err(FatGeometryError::FatSize);
        }
        let fat_start = volume_start
            .checked_add(reserved)
            .ok_or(FatGeometryError::Overflow)?;
        let root_dir_sectors = root_entries
            .checked_mul(32)
            .and_then(|bytes| bytes.checked_add(511))
            .ok_or(FatGeometryError::Overflow)?
            / 512;
        let fats_total = fat_sectors
            .checked_mul(u32::from(fat_count))
            .ok_or(FatGeometryError::Overflow)?;
        let data_start = fat_start
            .checked_add(fats_total)
            .and_then(|value| value.checked_add(root_dir_sectors))
            .ok_or(FatGeometryError::Overflow)?;
        let volume_end = volume_start
            .checked_add(total_sectors)
            .ok_or(FatGeometryError::Overflow)?;
        if data_start >= volume_end {
            return Err(FatGeometryError::InvalidRange);
        }
        let root_cluster = if fat16 == 0 {
            u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]])
        } else {
            0
        };
        if fat16 == 0 && root_cluster < 2 {
            return Err(FatGeometryError::InvalidRange);
        }
        Ok(Self {
            volume_start,
            volume_end,
            fat_start,
            fat_sectors,
            fat_count,
            data_start,
            sectors_per_cluster,
            root_cluster,
        })
    }

    pub const fn sectors_per_cluster(self) -> u8 {
        self.sectors_per_cluster
    }

    pub const fn root_cluster(self) -> u32 {
        self.root_cluster
    }

    pub fn classify(self, sector: u32) -> SectorPurpose {
        if sector < self.volume_start || sector >= self.volume_end {
            return SectorPurpose::OutsideVolume;
        }
        if sector < self.fat_start {
            return SectorPurpose::Reserved;
        }
        let fat_offset = sector - self.fat_start;
        let fats_total = self.fat_sectors * u32::from(self.fat_count);
        if fat_offset < fats_total {
            return SectorPurpose::Fat {
                copy: (fat_offset / self.fat_sectors) as u8,
            };
        }
        if sector < self.data_start {
            return SectorPurpose::Directory;
        }
        SectorPurpose::DataOther
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticRange {
    start: u32,
    end: u32,
    purpose: SectorPurpose,
}

pub struct SemanticSectorMap<const N: usize> {
    geometry: FatGeometry,
    ranges: [Option<SemanticRange>; N],
    len: usize,
}

impl<const N: usize> SemanticSectorMap<N> {
    pub const fn new(geometry: FatGeometry) -> Self {
        Self {
            geometry,
            ranges: [None; N],
            len: 0,
        }
    }

    pub fn add_range(
        &mut self,
        start: u32,
        sectors: u32,
        purpose: SectorPurpose,
    ) -> Result<(), FatGeometryError> {
        if sectors == 0
            || !matches!(
                purpose,
                SectorPurpose::Directory | SectorPurpose::Journal | SectorPurpose::Payload
            )
            || self.geometry.classify(start) != SectorPurpose::DataOther
        {
            return Err(FatGeometryError::InvalidRange);
        }
        let end = start
            .checked_add(sectors)
            .ok_or(FatGeometryError::Overflow)?;
        if self.geometry.classify(end - 1) != SectorPurpose::DataOther {
            return Err(FatGeometryError::InvalidRange);
        }
        if self.ranges[..self.len]
            .iter()
            .flatten()
            .any(|range| range.start == start && range.end == end && range.purpose == purpose)
        {
            return Ok(());
        }
        let slot = self
            .ranges
            .get_mut(self.len)
            .ok_or(FatGeometryError::Capacity)?;
        *slot = Some(SemanticRange {
            start,
            end,
            purpose,
        });
        self.len += 1;
        Ok(())
    }

    pub fn classify(&self, sector: u32) -> SectorPurpose {
        let physical = self.geometry.classify(sector);
        if physical != SectorPurpose::DataOther {
            return physical;
        }
        self.ranges[..self.len]
            .iter()
            .flatten()
            .rev()
            .find(|range| sector >= range.start && sector < range.end)
            .map_or(SectorPurpose::DataOther, |range| range.purpose)
    }
}
