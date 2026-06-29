# BinBook Roadmap

## X4 ADC input refactor

Status: evidence-gated.

Current firmware uses synchronous one-shot ADC reads, a 50 ms Embassy timer, and one global 100 ms cooldown. The candidate architecture uses `Adc::into_async()`, interrupt-completed `read_oneshot().await`, 20 ms periodic sampling, independent stable-candidate state for each ADC ladder, and 30 ms debounce matching the verified SquidScript/X4 reference.

ADC conversion completion can be interrupt-driven, but resistor-ladder button detection still requires periodic sampling; GPIO edges cannot distinguish ladder voltages reliably. Continuous ADC/DMA is not the default because it adds power and RAM complexity without removing debounce.

Do not implement this refactor until serial/camera stress plus physical input logs localize the problem to ADC sampling or debounce. Acceptance requires rapid mixed-direction host sequences, calibrated threshold tests, queue/drop evidence, pinned builds, flash, serial capture, and live physical-button confirmation.
