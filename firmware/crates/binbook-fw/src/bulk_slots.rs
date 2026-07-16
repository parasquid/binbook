use binbook_diagnostic_protocol::TransferDirection;

use crate::bulk_slot_types::BulkSlot;
pub use crate::bulk_slot_types::{
    BulkSlotDescriptor, BulkSlotError, BulkSlotState, TRANSFER_DATA_BYTES, TRANSFER_POOL_RAM_MAX,
    TRANSFER_SLOT_COUNT,
};

pub struct BulkSlotPool {
    slots: [BulkSlot; TRANSFER_SLOT_COUNT],
    generation: u32,
    peak_occupied: u8,
}

const _: () = assert!(core::mem::size_of::<BulkSlotPool>() <= TRANSFER_POOL_RAM_MAX);

impl BulkSlotPool {
    pub const fn new() -> Self {
        Self {
            slots: [BulkSlot::new(); TRANSFER_SLOT_COUNT],
            generation: 0,
            peak_occupied: 0,
        }
    }

    pub fn begin_session(&mut self) -> u32 {
        self.invalidate()
    }

    pub fn abort_session(&mut self) -> u32 {
        self.invalidate()
    }

    fn invalidate(&mut self) -> u32 {
        self.generation = self.generation.wrapping_add(1).max(1);
        for slot in &mut self.slots {
            slot.reset(self.generation);
        }
        self.peak_occupied = 0;
        self.generation
    }

    pub fn claim_upload(
        &mut self,
        offset: u32,
        len: u16,
    ) -> Result<BulkSlotDescriptor, BulkSlotError> {
        if len == 0 || usize::from(len) > TRANSFER_DATA_BYTES {
            return Err(BulkSlotError::Length);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.state == BulkSlotState::Free)
            .ok_or(BulkSlotError::Full)?;
        let slot = &mut self.slots[index];
        slot.generation = self.generation;
        slot.offset = offset;
        slot.len = len;
        slot.direction = TransferDirection::Upload;
        slot.state = BulkSlotState::RxReady;
        let descriptor = slot.descriptor(index);
        self.update_peak();
        Ok(descriptor)
    }

    pub fn take_upload_for_storage(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<(), BulkSlotError> {
        let slot = self.validate_mut(descriptor, BulkSlotState::RxReady)?;
        if slot.direction != TransferDirection::Upload {
            return Err(BulkSlotError::WrongState);
        }
        slot.state = BulkSlotState::StorageOwned;
        Ok(())
    }

    pub fn complete_upload(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<u32, BulkSlotError> {
        let generation = self.generation;
        let slot = self.validate_mut(descriptor, BulkSlotState::StorageOwned)?;
        if slot.direction != TransferDirection::Upload {
            return Err(BulkSlotError::WrongState);
        }
        let durable = slot
            .offset
            .checked_add(u32::from(slot.len))
            .ok_or(BulkSlotError::Length)?;
        slot.reset(generation);
        Ok(durable)
    }

    pub fn reserve_read(
        &mut self,
        offset: u32,
        len: u16,
    ) -> Result<BulkSlotDescriptor, BulkSlotError> {
        if len == 0 || usize::from(len) > TRANSFER_DATA_BYTES {
            return Err(BulkSlotError::Length);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.state == BulkSlotState::Free)
            .ok_or(BulkSlotError::Full)?;
        let slot = &mut self.slots[index];
        slot.generation = self.generation;
        slot.offset = offset;
        slot.len = len;
        slot.direction = TransferDirection::Readback;
        slot.state = BulkSlotState::StorageOwned;
        let descriptor = slot.descriptor(index);
        self.update_peak();
        Ok(descriptor)
    }

    pub fn complete_read(&mut self, descriptor: BulkSlotDescriptor) -> Result<(), BulkSlotError> {
        self.transition_read(
            descriptor,
            BulkSlotState::StorageOwned,
            BulkSlotState::TxReady,
        )
    }

    pub fn mark_awaiting_ack(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<(), BulkSlotError> {
        self.transition_read(
            descriptor,
            BulkSlotState::TxReady,
            BulkSlotState::AwaitingAck,
        )
    }

    pub fn ack_read(&mut self, descriptor: BulkSlotDescriptor) -> Result<(), BulkSlotError> {
        let generation = self.generation;
        let slot = self.validate_mut(descriptor, BulkSlotState::AwaitingAck)?;
        if slot.direction != TransferDirection::Readback {
            return Err(BulkSlotError::WrongState);
        }
        slot.reset(generation);
        Ok(())
    }

    fn transition_read(
        &mut self,
        descriptor: BulkSlotDescriptor,
        from: BulkSlotState,
        to: BulkSlotState,
    ) -> Result<(), BulkSlotError> {
        let slot = self.validate_mut(descriptor, from)?;
        if slot.direction != TransferDirection::Readback {
            return Err(BulkSlotError::WrongState);
        }
        slot.state = to;
        Ok(())
    }

    pub fn state(&self, slot: u8) -> Result<BulkSlotState, BulkSlotError> {
        self.slots
            .get(usize::from(slot))
            .map(|slot| slot.state)
            .ok_or(BulkSlotError::InvalidSlot)
    }

    pub fn occupied(&self) -> u8 {
        self.slots
            .iter()
            .filter(|slot| slot.state != BulkSlotState::Free)
            .count() as u8
    }

    pub const fn peak_occupied(&self) -> u8 {
        self.peak_occupied
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }

    fn update_peak(&mut self) {
        self.peak_occupied = self.peak_occupied.max(self.occupied());
    }

    fn validate(&self, descriptor: BulkSlotDescriptor) -> Result<&BulkSlot, BulkSlotError> {
        if descriptor.generation != self.generation {
            return Err(BulkSlotError::StaleGeneration);
        }
        let slot = self
            .slots
            .get(usize::from(descriptor.slot))
            .ok_or(BulkSlotError::InvalidSlot)?;
        if slot.descriptor(usize::from(descriptor.slot)) != descriptor {
            return Err(BulkSlotError::DescriptorMismatch);
        }
        Ok(slot)
    }

    fn validate_mut(
        &mut self,
        descriptor: BulkSlotDescriptor,
        state: BulkSlotState,
    ) -> Result<&mut BulkSlot, BulkSlotError> {
        self.validate(descriptor)?;
        let slot = &mut self.slots[usize::from(descriptor.slot)];
        if slot.state != state {
            return Err(BulkSlotError::WrongState);
        }
        Ok(slot)
    }
}

impl Default for BulkSlotPool {
    fn default() -> Self {
        Self::new()
    }
}
