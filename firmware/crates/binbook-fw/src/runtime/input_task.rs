use portable_atomic::Ordering;

use esp_hal::analog::adc::{Adc, AdcCalBasic, AdcConfig, Attenuation};
use esp_hal::gpio::{Input, InputConfig, Pull};

use binbook_fw::{
    async_refresh::{button_to_request, DisplayRequest, INPUT_POLL_INTERVAL_MS},
    input::{self, InputState},
    runtime_engine::{RequestEnqueueStatus, RuntimeEvent, RuntimeEventKind, RuntimeRequestKind},
};

use super::{
    PowerAckReceiver, PowerRequestSender, RequestSender, DISPLAY_MODE, REQUEST_EPOCH,
    RUNTIME_EVENT_CHANNEL,
};

#[embassy_executor::task]
pub(super) async fn input_task(
    adc1: esp_hal::peripherals::ADC1<'static>,
    gpio1: esp_hal::peripherals::GPIO1<'static>,
    gpio2: esp_hal::peripherals::GPIO2<'static>,
    request_tx: RequestSender,
    mut gpio3: esp_hal::peripherals::GPIO3<'static>,
    lpwr: esp_hal::peripherals::LPWR<'static>,
    power_request_tx: PowerRequestSender,
    power_ack_rx: PowerAckReceiver,
) {
    let mut adc_config = AdcConfig::new();
    let mut ch1_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio1, Attenuation::_11dB);
    let mut ch2_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio2, Attenuation::_11dB);
    let mut adc = Adc::new(adc1, adc_config).into_async();
    let mut input_state = InputState::new();
    let power_pin = Input::new(gpio3.reborrow(), InputConfig::default().with_pull(Pull::Up));
    let mut power_hold = binbook_fw::power::PowerHold::new();
    let mut rtc = esp_hal::rtc_cntl::Rtc::new(lpwr);
    let mut tick: u64 = 0;

    loop {
        embassy_time::Timer::after_millis(INPUT_POLL_INTERVAL_MS).await;
        tick = tick.saturating_add(INPUT_POLL_INTERVAL_MS);

        let ch1 = adc.read_oneshot(&mut ch1_pin).await;
        let ch2 = adc.read_oneshot(&mut ch2_pin).await;

        let outcome = input_state.poll_raw_detailed(ch1, ch2, tick);
        let timestamp_ms = embassy_time::Instant::now().as_millis();
        match power_hold.update(power_pin.is_low(), timestamp_ms) {
            binbook_fw::power::PowerAction::FullRefresh => {
                let request = binbook_fw::power::PowerRequest::FullRefresh;
                power_request_tx.send(request).await;
                let _completion = binbook_fw::power::PowerRequestCompletion::new(
                    request,
                    power_ack_rx.receive().await,
                );
            }
            binbook_fw::power::PowerAction::PrepareSleep => {
                let request = binbook_fw::power::PowerRequest::PrepareSleep;
                power_request_tx.send(request).await;
                let completion = binbook_fw::power::PowerRequestCompletion::new(
                    request,
                    power_ack_rx.receive().await,
                );
                if completion.may_enter_cpu_deep_sleep() {
                    while !binbook_fw::power::PowerReleaseGate::ready(power_pin.is_low()) {
                        embassy_time::Timer::after_millis(INPUT_POLL_INTERVAL_MS).await;
                    }
                    drop(power_pin);
                    let mut wakeup_pins: [(
                        &mut dyn esp_hal::gpio::RtcPinWithResistors,
                        esp_hal::rtc_cntl::sleep::WakeupLevel,
                    ); 1] = [(&mut gpio3, esp_hal::rtc_cntl::sleep::WakeupLevel::Low)];
                    let power_wakeup =
                        esp_hal::rtc_cntl::sleep::RtcioWakeupSource::new(&mut wakeup_pins);
                    rtc.sleep_deep(&[&power_wakeup]);
                }
            }
            binbook_fw::power::PowerAction::None => {}
        }
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
            let mode = DISPLAY_MODE.load(Ordering::Relaxed);
            if let Some(request) = button_to_request(button, mode) {
                let request_kind = RuntimeRequestKind::from_request(&request);
                let request_sequence = binbook_fw::runtime_engine::request_sequence(&request);
                if request_tx.try_send(request).is_ok() {
                    RUNTIME_EVENT_CHANNEL
                        .sender()
                        .send(RuntimeEvent {
                            timestamp_ms,
                            kind: RuntimeEventKind::RequestEnqueue {
                                kind: request_kind,
                                sequence: request_sequence,
                                status: RequestEnqueueStatus::Ok,
                            },
                        })
                        .await;
                    REQUEST_EPOCH.fetch_add(1, Ordering::AcqRel);
                } else {
                    RUNTIME_EVENT_CHANNEL
                        .sender()
                        .send(RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: RuntimeEventKind::RequestEnqueue {
                                kind: request_kind,
                                sequence: request_sequence,
                                status: RequestEnqueueStatus::Full,
                            },
                        })
                        .await;
                    if let DisplayRequest::Turn { turn, .. } = request {
                        RUNTIME_EVENT_CHANNEL
                            .sender()
                            .send(RuntimeEvent {
                                timestamp_ms: embassy_time::Instant::now().as_millis(),
                                kind: RuntimeEventKind::TurnDropped { turn },
                            })
                            .await;
                    }
                }
            } else {
                RUNTIME_EVENT_CHANNEL
                    .sender()
                    .send(RuntimeEvent {
                        timestamp_ms,
                        kind: RuntimeEventKind::RequestEnqueue {
                            kind: RuntimeRequestKind::Turn,
                            sequence: None,
                            status: RequestEnqueueStatus::Unmapped,
                        },
                    })
                    .await;
            }
        }
    }
}
