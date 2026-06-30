use portable_atomic::Ordering;

use esp_hal::analog::adc::{Adc, AdcCalBasic, AdcConfig, Attenuation};

use binbook_fw::{
    async_refresh::{DisplayRequest, INPUT_POLL_INTERVAL_MS},
    input::{self, InputState},
    runtime_engine::{RuntimeEvent, RuntimeEventKind},
};

use super::{RequestSender, REQUEST_EPOCH, RUNTIME_EVENT_CHANNEL};

#[embassy_executor::task]
pub(super) async fn input_task(
    adc1: esp_hal::peripherals::ADC1<'static>,
    gpio1: esp_hal::peripherals::GPIO1<'static>,
    gpio2: esp_hal::peripherals::GPIO2<'static>,
    request_tx: RequestSender,
) {
    let mut adc_config = AdcConfig::new();
    let mut ch1_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio1, Attenuation::_11dB);
    let mut ch2_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio2, Attenuation::_11dB);
    let mut adc = Adc::new(adc1, adc_config);
    let mut input_state = InputState::new();
    let mut tick: u64 = 0;

    loop {
        embassy_time::Timer::after_millis(INPUT_POLL_INTERVAL_MS).await;
        tick = tick.saturating_add(INPUT_POLL_INTERVAL_MS);

        let ch1 = loop {
            match adc.read_oneshot(&mut ch1_pin) {
                Ok(value) => break value,
                Err(nb::Error::WouldBlock) => {}
                Err(nb::Error::Other(())) => break 0,
            }
        };
        let ch2 = loop {
            match adc.read_oneshot(&mut ch2_pin) {
                Ok(value) => break value,
                Err(nb::Error::WouldBlock) => {}
                Err(nb::Error::Other(())) => break 0,
            }
        };

        let outcome = input_state.poll_raw_detailed(ch1, ch2, tick);
        let timestamp_ms = embassy_time::Instant::now().as_millis();
        if outcome.previous != outcome.observed {
            RUNTIME_EVENT_CHANNEL
                .sender()
                .send(RuntimeEvent {
                    timestamp_ms,
                    kind: RuntimeEventKind::InputTransition {
                        ch1,
                        ch2,
                        observed: outcome.observed,
                    },
                })
                .await;
        }
        if outcome.decision != input::InputDecision::Unchanged {
            RUNTIME_EVENT_CHANNEL
                .sender()
                .send(RuntimeEvent {
                    timestamp_ms,
                    kind: RuntimeEventKind::InputDecision {
                        observed: outcome.observed,
                        decision: outcome.decision,
                        elapsed_ms: outcome.elapsed_since_last_press_ms,
                    },
                })
                .await;
        }
        if let input::InputDecision::Press(button) = outcome.decision {
            if let Some(turn) = input::page_turn_for_button(button) {
                if request_tx
                    .try_send(DisplayRequest::Turn {
                        turn,
                        completion_sequence: None,
                    })
                    .is_ok()
                {
                    REQUEST_EPOCH.fetch_add(1, Ordering::AcqRel);
                } else {
                    RUNTIME_EVENT_CHANNEL
                        .sender()
                        .send(RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: RuntimeEventKind::TurnDropped { turn },
                        })
                        .await;
                }
            }
        }
    }
}
