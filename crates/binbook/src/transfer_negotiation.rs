use binbook_diagnostic_protocol::{
    TransferDirection, TransferLimits, CAP_STORAGE_BULK_V1, CAP_STORAGE_READ_WINDOW_2,
    CAP_STORAGE_TRANSFER_LIMITS, CAP_STORAGE_UPLOAD_WINDOW_2, TRANSFER_LIMIT_READ_BULK,
    TRANSFER_LIMIT_UPLOAD_BULK,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedMode {
    Auto,
    Legacy,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedMode {
    Legacy,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedWindow {
    Auto,
    One,
    Two,
    Four,
    Eight,
}

impl RequestedWindow {
    const fn value(self) -> Option<u8> {
        match self {
            Self::Auto => None,
            Self::One => Some(1),
            Self::Two => Some(2),
            Self::Four => Some(4),
            Self::Eight => Some(8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferSelection {
    pub capabilities: u32,
    pub mode: SelectedMode,
    pub window: u8,
    pub chunk_bytes: u16,
    pub idle_timeout_ms: u32,
    pub limits: TransferLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TransferNegotiationError {
    #[error("peer advertised transfer limits but did not return them")]
    LimitsMissing,
    #[error("bulk transfer is not supported for this direction")]
    BulkUnsupported,
    #[error("transfer direction is invalid")]
    InvalidDirection,
}

pub fn negotiate_transfer(
    capabilities: u32,
    limits: Option<TransferLimits>,
    direction: TransferDirection,
    requested_mode: RequestedMode,
    requested_window: RequestedWindow,
) -> Result<TransferSelection, TransferNegotiationError> {
    if direction == TransferDirection::Unspecified {
        return Err(TransferNegotiationError::InvalidDirection);
    }
    if capabilities & CAP_STORAGE_TRANSFER_LIMITS != 0 && limits.is_none() {
        return Err(TransferNegotiationError::LimitsMissing);
    }

    let direction_flag = match direction {
        TransferDirection::Upload => TRANSFER_LIMIT_UPLOAD_BULK,
        TransferDirection::Readback => TRANSFER_LIMIT_READ_BULK,
        TransferDirection::Unspecified => unreachable!(),
    };
    let bulk_supported = capabilities & CAP_STORAGE_BULK_V1 != 0
        && capabilities & CAP_STORAGE_TRANSFER_LIMITS != 0
        && limits.is_some_and(|value| value.flags & direction_flag != 0);
    let mode = match requested_mode {
        RequestedMode::Legacy => SelectedMode::Legacy,
        RequestedMode::Bulk if bulk_supported => SelectedMode::Bulk,
        RequestedMode::Bulk => return Err(TransferNegotiationError::BulkUnsupported),
        RequestedMode::Auto if bulk_supported => SelectedMode::Bulk,
        RequestedMode::Auto => SelectedMode::Legacy,
    };

    let legacy_capability = match direction {
        TransferDirection::Upload => CAP_STORAGE_UPLOAD_WINDOW_2,
        TransferDirection::Readback => CAP_STORAGE_READ_WINDOW_2,
        TransferDirection::Unspecified => unreachable!(),
    };
    let maximum_window = match (mode, limits) {
        (SelectedMode::Bulk, Some(value)) => value.bulk_slots,
        (SelectedMode::Legacy, Some(value)) => match direction {
            TransferDirection::Upload => value.legacy_upload_window,
            TransferDirection::Readback => value.legacy_read_window,
            TransferDirection::Unspecified => unreachable!(),
        },
        (SelectedMode::Legacy, None) if capabilities & legacy_capability != 0 => 2,
        (SelectedMode::Legacy, None) => 1,
        (SelectedMode::Bulk, None) => unreachable!(),
    };
    let window = requested_window
        .value()
        .unwrap_or(maximum_window)
        .min(maximum_window)
        .max(1);
    let effective_limits = limits.unwrap_or_else(|| legacy_limits(capabilities));
    Ok(TransferSelection {
        capabilities,
        mode,
        window,
        chunk_bytes: effective_limits.max_data_bytes,
        idle_timeout_ms: effective_limits.idle_timeout_ms,
        limits: effective_limits,
    })
}

fn legacy_limits(capabilities: u32) -> TransferLimits {
    let upload_window = if capabilities & CAP_STORAGE_UPLOAD_WINDOW_2 != 0 {
        2
    } else {
        1
    };
    let read_window = if capabilities & CAP_STORAGE_READ_WINDOW_2 != 0 {
        2
    } else {
        1
    };
    let largest_window = upload_window.max(read_window);
    let max_frame_bytes = binbook_diagnostic_protocol::MAX_FRAME_BYTES as u16;
    TransferLimits {
        flags: 0,
        max_data_bytes: 3_584,
        legacy_upload_window: upload_window,
        legacy_read_window: read_window,
        bulk_slots: 0,
        rx_capacity_bytes: max_frame_bytes.saturating_mul(u16::from(largest_window)),
        max_frame_bytes,
        idle_timeout_ms: 60_000,
    }
}
