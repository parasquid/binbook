#![cfg(feature = "serial-device")]

use binbook::{sd_write_trace_client, serial_transport::SerialSession};
use std::path::Path;

#[test]
#[ignore = "requires exclusive X4 serial access"]
fn hardware_sd_write_trace_clear_and_repeated_get_remain_responsive() {
    let port = std::env::var("BINBOOK_TEST_PORT").unwrap_or_else(|_| "/dev/ttyACM0".into());
    let mut serial = SerialSession::open(&port).unwrap();
    sd_write_trace_client::clear(&mut serial).unwrap();
    let first = sd_write_trace_client::get_all(&mut serial).unwrap();
    let second = sd_write_trace_client::get_all(&mut serial).unwrap();
    assert_eq!(first.records, second.records);
    assert!(!first.overflowed);
    assert_eq!(first.anomalies, 0);
}

#[test]
#[ignore = "requires exclusive X4 serial access and an existing capture"]
fn hardware_sd_write_trace_retrieves_existing_frozen_capture() {
    let port = std::env::var("BINBOOK_TEST_PORT").unwrap_or_else(|_| "/dev/ttyACM0".into());
    let output = std::env::var("BINBOOK_SD_TRACE_OUTPUT")
        .expect("BINBOOK_SD_TRACE_OUTPUT must name the JSON evidence path");
    let mut serial = SerialSession::open(&port).unwrap();
    sd_write_trace_client::freeze(&mut serial).unwrap();
    let capture = sd_write_trace_client::get_all(&mut serial).unwrap();
    capture.write_atomic(Path::new(&output)).unwrap();
    eprintln!(
        "records={} capacity={} overflowed={} anomalies={}",
        capture.records.len(),
        capture.capacity,
        capture.overflowed,
        capture.anomalies
    );
    assert!(!capture.records.is_empty());
}
