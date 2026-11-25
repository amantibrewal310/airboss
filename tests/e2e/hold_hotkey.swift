// Holds ⌃⌥N as a global hotkey, as another app would, so Airboss finds it taken at start-up.
//   swiftc -O -o /tmp/hold_hotkey tests/e2e/hold_hotkey.swift && /tmp/hold_hotkey
// Prints "holding" once registered (or "refused <status>"), then runs until killed.
import Carbon
import Foundation

var ref: EventHotKeyRef?
let id = EventHotKeyID(signature: OSType(0x5141_484B), id: 1)  // "QAHK"
let status = RegisterEventHotKey(
    UInt32(kVK_ANSI_N), UInt32(controlKey | optionKey), id, GetApplicationEventTarget(), 0, &ref)
print(status == noErr ? "holding" : "refused \(status)")
fflush(stdout)
RunLoop.main.run()
