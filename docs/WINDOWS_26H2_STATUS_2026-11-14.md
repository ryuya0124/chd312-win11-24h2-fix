# Windows 26H2 update check — 2026-11-14

The installed Windows 11 26H2 build is 26300.9974 (KB5132061). This was a read-only check of the connected CHD312; no driver switching, device removal, or monitor disable was performed.

Both analog and digital audio functions are currently using `wdma_usb.inf`, with PnP problem code 0. All 14 device-identity checks pass. The device still declares Audio/SubClass 03 in its IAD/compatible IDs.

`pnputil /enum-devices /instanceid <audio-function> /drivers` reports the following for both functions:

| Driver | Version | Rank | Status |
| --- | --- | --- | --- |
| `usbmidi2.inf` | 10.0.26100.9974 | 00FF2003 | Best Ranked |
| `wdma_usb.inf` | 10.0.26100.9974 | 00FF2005 | Outranked / Installed |

The current healthy audio binding therefore does not demonstrate that Windows has corrected automatic driver selection. The problematic MIDI candidate remains preferred over the working audio driver. Existing logs also show recent successful scans with no repair needed, but those scans inspect the already-selected audio driver; they do not test a fresh automatic binding.

Conclusion: it is not yet justified to retire the workaround based on this Windows update. Leave the working audio binding and monitor state unchanged. A future retirement check should verify clean automatic enumeration without the workaround and actual audio capture, with a recovery path available; that disruptive test was not performed here.

Microsoft still warns that selecting the MIDI driver for a non-MIDI audio device can prevent it from working, and documents a Plug and Play issue that can revert a chosen driver after updates:

- https://microsoft.github.io/MIDI/kb/how-to-switch-between-class-drivers/
- https://microsoft.github.io/MIDI/tools/miditroubleshooter/
