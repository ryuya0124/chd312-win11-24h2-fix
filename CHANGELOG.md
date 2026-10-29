# Changes

## 0.1.1

- Fix a discovery race after reconnect: an empty Windows device enumeration now receives bounded retries in event-triggered monitoring. `status` always queries current state without settling waits or retries.
- Replace the misleading `not connected` status with `not detected by Windows`. A connected USB device may still be initializing its Windows functions.
- When available, report read-only USB hub state at previously validated locations, including DeviceEnumerating and DeviceReset. These hints never authorize a driver change.
- Keep all 14 identity checks and the MIDI-only repair policy. Correct USB Audio and unknown driver bindings remain untouched.

Observed on one device: the hub reported connected VID/PID 04B4:00F8 while Windows function enumeration was empty; about 0.3 seconds later, all 14 checks passed. This update handles the observed discovery race; it does not guarantee a fix for every USB initialization delay or hardware connection failure.

## 0.1.0

- Initial Rust portable and installer prerelease.
