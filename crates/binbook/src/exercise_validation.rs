use binbook_diagnostic_protocol::{
    LogRecordPayload, StatusPayload, EVT_BW_BASE_SYNC_CANCELLED, EVT_BW_BASE_SYNC_COMPLETE,
    EVT_BW_BASE_SYNC_START, EVT_DISPLAY_ERROR, EVT_GRAY_OVERLAY_ACTIVATE,
    EVT_GRAY_OVERLAY_CANCELLED, EVT_GRAY_OVERLAY_COMPLETE, EVT_GRAY_OVERLAY_START,
    EVT_REFRESH_DECISION, EVT_RENDER_FAILURE, EVT_TURN_DEQUEUED, EVT_TURN_DROPPED,
    EVT_WAVEFORM_SELECTED,
};

pub fn validate_staged_gray_evidence(
    status: StatusPayload,
    records: &[LogRecordPayload],
) -> Result<(), String> {
    if status.current_page != 3 {
        return Err(format!("final page must be 3, got {}", status.current_page));
    }
    if status.dropped_log_count != 0 || status.protocol_error_count != 0 {
        return Err("diagnostic counters must remain zero".into());
    }
    if status.last_error != 0 {
        return Err(format!(
            "last_error must remain zero, got {}",
            status.last_error
        ));
    }
    if records
        .iter()
        .any(|record| record.event == EVT_TURN_DROPPED)
    {
        return Err("TURN_DROPPED observed".into());
    }
    if records
        .iter()
        .any(|record| matches!(record.event, EVT_RENDER_FAILURE | EVT_DISPLAY_ERROR))
    {
        return Err("display failure observed".into());
    }

    let prime = find_completion(records, 5, 1)?;
    let overlay_start_index = records
        .iter()
        .position(|record| record.event == EVT_GRAY_OVERLAY_START && record.arg0 == 1)
        .ok_or_else(|| "missing staged overlay start for page 1".to_string())?;
    if records[overlay_start_index].tick_ms < prime.tick_ms.saturating_add(350) {
        return Err("staged overlay started less than 350 ms after BW completion".into());
    }
    let waveform_index = records
        .iter()
        .position(|record| {
            record.event == EVT_WAVEFORM_SELECTED && record.arg0 == 2 && record.arg1 == 1
        })
        .ok_or_else(|| "missing waveform hint 2 / LUT revision 1 evidence".to_string())?;
    let activate_index = records
        .iter()
        .position(|record| record.event == EVT_GRAY_OVERLAY_ACTIVATE && record.arg0 == 1)
        .ok_or_else(|| "missing staged overlay activation for page 1".to_string())?;
    let overlay_complete_index = records
        .iter()
        .position(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE && record.arg0 == 1)
        .ok_or_else(|| "missing staged overlay completion for page 1".to_string())?;
    if !(overlay_start_index < waveform_index
        && waveform_index < activate_index
        && activate_index < overlay_complete_index)
    {
        return Err("staged overlay events are missing or reordered".into());
    }
    if records.iter().any(|record| {
        record.event == EVT_REFRESH_DECISION
            && [record.arg0, record.arg1, record.arg2]
                .iter()
                .any(|value| matches!(*value, 0xF7 | 0xC7))
    }) {
        return Err("full or absolute-grayscale activation observed".into());
    }

    let queued = [
        completion_index(records, 7, 2)?,
        completion_index(records, 8, 3)?,
        completion_index(records, 9, 2)?,
    ];
    if !(queued[0] < queued[1] && queued[1] < queued[2]) {
        return Err("queued pages were not completed in exact order 2,3,2".into());
    }

    let cancel_index = records
        .iter()
        .position(|record| record.event == EVT_GRAY_OVERLAY_CANCELLED && record.arg0 == 2)
        .ok_or_else(|| "missing cancellation of the queued page-2 overlay".to_string())?;
    let restarted_index = records
        .iter()
        .enumerate()
        .skip(cancel_index + 1)
        .find(|(_, record)| record.event == EVT_GRAY_OVERLAY_START && record.arg0 == 2)
        .map(|(index, _)| index)
        .unwrap_or(records.len());
    if records[cancel_index + 1..restarted_index]
        .iter()
        .any(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE && record.arg0 == 2)
    {
        return Err("canceled page emitted a grayscale completion".into());
    }

    let sync_start_index = records
        .iter()
        .position(|record| record.event == EVT_BW_BASE_SYNC_START && record.arg0 == 2)
        .ok_or_else(|| "missing BW base-sync start for page 2".to_string())?;
    let sync_complete_index = records
        .iter()
        .enumerate()
        .skip(sync_start_index + 1)
        .find(|(_, record)| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 2)
        .map(|(index, _)| index)
        .ok_or_else(|| "missing BW base-sync completion for page 2".to_string())?;
    if records[sync_start_index..=sync_complete_index]
        .iter()
        .any(|record| {
            matches!(
                record.event,
                EVT_GRAY_OVERLAY_ACTIVATE | EVT_REFRESH_DECISION
            )
        })
    {
        return Err("visible activation occurred during background BW base sync".into());
    }
    if records.iter().any(|record| {
        record.event == EVT_BW_BASE_SYNC_CANCELLED
            && record.arg0 == 2
            && record.sequence > records[sync_complete_index].sequence
    }) {
        return Err("completed base sync was later reported canceled".into());
    }
    find_completion(records, 11, 3)?;
    Ok(())
}

fn find_completion(
    records: &[LogRecordPayload],
    sequence: i32,
    page: i32,
) -> Result<&LogRecordPayload, String> {
    records
        .iter()
        .find(|record| {
            record.event == EVT_TURN_DEQUEUED && record.arg0 == sequence && record.arg1 == page
        })
        .ok_or_else(|| format!("missing completion sequence={sequence} page={page}"))
}

fn completion_index(
    records: &[LogRecordPayload],
    sequence: i32,
    page: i32,
) -> Result<usize, String> {
    records
        .iter()
        .position(|record| {
            record.event == EVT_TURN_DEQUEUED && record.arg0 == sequence && record.arg1 == page
        })
        .ok_or_else(|| format!("missing completion sequence={sequence} page={page}"))
}
