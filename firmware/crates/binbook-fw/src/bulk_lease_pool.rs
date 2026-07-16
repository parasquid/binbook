use crate::{
    bulk_slots::{
        BulkSlotDescriptor, TRANSFER_DATA_BYTES, TRANSFER_POOL_RAM_MAX, TRANSFER_SLOT_COUNT,
    },
    storage_owner::{BufferLease, BufferSlot},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkLeaseError {
    InvalidSlot,
    StaleGeneration,
    Length,
    Unavailable,
    WrongLease,
}

pub struct BulkLeasePool {
    buffers: [Option<&'static mut [u8; TRANSFER_DATA_BYTES]>; TRANSFER_SLOT_COUNT],
    generation: u32,
}

const _: () = assert!(TRANSFER_SLOT_COUNT * TRANSFER_DATA_BYTES <= TRANSFER_POOL_RAM_MAX);
const _: () = assert!(core::mem::size_of::<BulkLeasePool>() <= 64);

impl BulkLeasePool {
    pub fn new(buffers: [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT]) -> Self {
        Self {
            buffers: buffers.map(Some),
            generation: 0,
        }
    }

    pub fn begin_session(&mut self, generation: u32) -> Result<(), BulkLeaseError> {
        if generation == 0 || self.buffers.iter().any(Option::is_none) {
            return Err(BulkLeaseError::Unavailable);
        }
        self.generation = generation;
        Ok(())
    }

    pub fn invalidate(&mut self, generation: u32) {
        self.generation = generation.max(1);
    }

    pub fn write(
        &mut self,
        descriptor: BulkSlotDescriptor,
        data: &[u8],
    ) -> Result<(), BulkLeaseError> {
        self.validate(descriptor)?;
        if data.len() != usize::from(descriptor.len) {
            return Err(BulkLeaseError::Length);
        }
        self.buffer_mut(descriptor)?[..data.len()].copy_from_slice(data);
        Ok(())
    }

    pub fn bytes(&self, descriptor: BulkSlotDescriptor) -> Result<&[u8], BulkLeaseError> {
        self.validate(descriptor)?;
        let buffer = self
            .buffers
            .get(usize::from(descriptor.slot))
            .and_then(Option::as_deref)
            .ok_or(BulkLeaseError::Unavailable)?;
        Ok(&buffer[..usize::from(descriptor.len)])
    }

    pub fn take(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<BufferLease<'static>, BulkLeaseError> {
        self.validate(descriptor)?;
        let index = usize::from(descriptor.slot);
        let buffer = self.buffers[index]
            .take()
            .ok_or(BulkLeaseError::Unavailable)?;
        let slot = BufferSlot::bulk(descriptor.slot).ok_or(BulkLeaseError::InvalidSlot)?;
        Ok(BufferLease::new(slot, buffer))
    }

    pub fn restore(
        &mut self,
        descriptor: BulkSlotDescriptor,
        lease: BufferLease<'static>,
    ) -> Result<(), BulkLeaseError> {
        let index = usize::from(descriptor.slot);
        if index >= TRANSFER_SLOT_COUNT
            || lease.slot().bulk_index() != Some(descriptor.slot)
            || self.buffers[index].is_some()
        {
            return Err(BulkLeaseError::WrongLease);
        }
        let bytes: &'static mut [u8; TRANSFER_DATA_BYTES] = lease
            .into_bytes()
            .try_into()
            .map_err(|_| BulkLeaseError::Length)?;
        self.buffers[index] = Some(bytes);
        Ok(())
    }

    pub fn available(&self) -> u8 {
        self.buffers
            .iter()
            .filter(|buffer| buffer.is_some())
            .count() as u8
    }

    fn buffer_mut(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<&mut [u8; TRANSFER_DATA_BYTES], BulkLeaseError> {
        self.buffers
            .get_mut(usize::from(descriptor.slot))
            .and_then(Option::as_deref_mut)
            .ok_or(BulkLeaseError::Unavailable)
    }

    fn validate(&self, descriptor: BulkSlotDescriptor) -> Result<(), BulkLeaseError> {
        if usize::from(descriptor.slot) >= TRANSFER_SLOT_COUNT {
            return Err(BulkLeaseError::InvalidSlot);
        }
        if descriptor.generation != self.generation {
            return Err(BulkLeaseError::StaleGeneration);
        }
        if descriptor.len == 0 || usize::from(descriptor.len) > TRANSFER_DATA_BYTES {
            return Err(BulkLeaseError::Length);
        }
        Ok(())
    }
}
