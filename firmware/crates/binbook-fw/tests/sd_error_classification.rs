#![cfg(feature = "sd-storage")]

use binbook_fw::storage::{classify_device_error, SdError};
use embedded_sdmmc::sdcard::Error;

#[test]
fn card_not_found_is_distinct_from_other_sd_device_failures() {
    assert!(matches!(
        classify_device_error(Error::CardNotFound),
        SdError::NoCard
    ));

    for error in [
        Error::Transport,
        Error::TimeoutReadBuffer,
        Error::TimeoutCommand(0),
        Error::CrcError(0x1234, 0x5678),
        Error::ReadError,
        Error::BadState,
    ] {
        assert!(matches!(classify_device_error(error), SdError::Sdmmc));
    }
}
