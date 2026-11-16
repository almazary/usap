# Usap

**Turn an Android phone into a trackpad for your Mac that feels close to a real Mac trackpad.**

[Bahasa Indonesia](README.md)

Usap has two parts: an Android app that reads finger positions on the phone screen, and a small agent on the Mac
that turns those touches into pointer movement, clicks, scrolling and gestures. All gesture logic (pointer
acceleration, scroll momentum, tap detection) runs on the Mac, so the phone only sends finger positions.

- Pointer acceleration: move slowly for precision, quickly to cross the screen.
- Smooth two-finger scrolling with momentum after you lift your fingers. It uses native macOS scroll events,
  so apps treat it like a real trackpad.
- Tap to click, two-finger tap to right-click, three-finger gestures for Mission Control and switching desktops.
- Select text and drag in a way that resembles the Mac trackpad.
- Four ways to connect: USB cable (no developer mode needed), Bluetooth, Wi-Fi and USB debugging.
- Haptic feedback on the phone when you click, and the phone's battery level on the trackpad screen.

The agent is written in Rust. The Android app is Kotlin with no extra libraries.

> Status: used daily by its author, but not yet tested on many other devices.
> macOS only for now. Windows support is planned.
> The app's interface and the agent's messages are in Indonesian.

## Why Usap exists

I spend a lot of time in front of my laptop, from 6 in the morning until midnight. Not all of it is work, but
my hand is on a mouse or trackpad for most of that time. I used a mouse for years without any problem. Then I
got used to the Mac trackpad and found it far more comfortable. Going back to a mouse, my hand often got sore:
moving a mouse takes effort from the arm and wrist, while a trackpad only needs your fingers.

The catch is that I often use an external keyboard. The MacBook gets pushed back behind the keyboard, its
trackpad is out of reach, and the usual partner for an external keyboard is a mouse. An Android phone is almost
always on my desk, so Usap turns it into a trackpad that sits next to the keyboard. The goal is to feel as close
to a Mac trackpad as possible: the pointer goes farther when you move faster, scrolling keeps gliding after you
lift your fingers, and the gestures are the same.

## Requirements

| | Minimum | Tested on |
|---|---|---|
| Mac | macOS 11 Big Sur | macOS 26, Apple Silicon |
| Phone | Android 8.0 (API 26) | TECNO CM5, Android 16 |

To build from source: Rust (stable) for the agent, JDK 17 and Android SDK 34 for the app.

## Install

### 1. Agent on the Mac

```sh
git clone https://github.com/almazary/usap.git
cd usap/agent
cargo build --release
cp target/release/usap-agent ~/.local/bin/   # or any folder on your PATH
```

### 2. App on the phone

Build the APK and install it:

```sh
./build-apk.sh                       # produces usap-<version>-debug.apk
adb install -r usap-0.8-debug.apk
```

Without `adb`, copy the APK to the phone and open it from a file manager. Android will ask you to allow
installing apps from unknown sources.

See [Building from source](#building-from-source) for details.

### 3. First run

```sh
usap-agent
```

macOS asks for a few permissions for the terminal app that runs the agent (Terminal, iTerm, Alacritty and so on).
Allow all of them:

- **Accessibility** (System Settings → Privacy & Security → Accessibility): required so the agent can move the
  pointer. Restart the agent after granting it.
- **Bluetooth**: for the Bluetooth connection.
- **Local Network**, and the firewall if it is on: for the Wi-Fi connection.

The agent prints a **6-digit code** in the terminal. It is saved in `~/.config/usap/code`, so it stays the same
across runs. Create a new one with `usap-agent --new-code`.

You don't need to run the agent from the project folder. Keep its terminal open while you use Usap.

## Connecting the phone

Pick one. The latency figures were measured on the author's Mac and phone.

| Connection | How | Typical latency | Notes |
|---|---|---|---|
| **USB cable** (Android Open Accessory) | Plug the phone into the Mac. Usap opens and connects by itself. | 2–3 ms | Most accurate. No developer mode, no code. |
| **Bluetooth LE** | On the phone, tap *Sambungkan lewat Bluetooth* (connect over Bluetooth). | about 10 ms, steady | Wireless. Works alongside a Bluetooth keyboard. |
| **Wi-Fi** | Tap *Cari komputer* (find computer), pick the Mac, enter the code. | 5 ms to hundreds of ms | Depends heavily on your Wi-Fi. See [Troubleshooting](#troubleshooting). |
| **USB debugging** (adb) | Enable USB debugging, plug in, tap *Kabel lewat USB debugging*. | 3–8 ms | Fallback. The agent runs `adb reverse tcp:50505 tcp:50505` itself. |

How the USB cable mode works: the agent looks for an Android phone on USB and asks it to switch to the "Usap"
accessory mode. Android asks whether Usap may open for this accessory; tick *always* so it doesn't ask again.
While the phone is in this mode, file transfer (MTP) is unavailable. Unplug the cable to go back to normal, or
run the agent with `--no-aoa` if you don't want this mode at all.

On the trackpad screen, the phone's back button disconnects.

## Gestures

| Gesture | Action |
|---|---|
| move 1 finger | move the pointer. The faster you move, the farther it goes. |
| tap | click. Tap twice to double-click, three times to triple-click. |
| hold the index finger still, then slide the middle finger | select text or drag (windows, tabs, files). The phone vibrates when the click is pressed. You can lift the middle finger and put it down again to extend the selection. Lift the index finger to finish. |
| move 2 fingers | smooth scrolling, with momentum after you lift |
| tap with 2 fingers | right-click |
| 3 fingers up | Mission Control |
| 3 fingers down | App Exposé |
| 3 fingers left or right | switch desktops (Spaces) |

The select gesture mimics the Mac trackpad, where the thumb presses the trackpad down while the index finger
moves. So that it doesn't get confused with scrolling, the click is pressed only when the index finger has been
still for a moment before the middle finger lands, the two fingers are close together, and the index finger
doesn't move along.

Two other drag gestures exist but are off by default, because both trigger too easily by accident:

- `hold_drag`: hold one finger still until the phone vibrates, then move.
- `tap_drag`: tap, then immediately put the finger down again and move.

Switching desktops uses the Ctrl+← and Ctrl+→ shortcuts, and Mission Control uses Ctrl+↑ and Ctrl+↓. If you
changed these in System Settings → Keyboard → Keyboard Shortcuts, the three-finger gestures won't work.

## Settings

Settings live in `~/.config/usap/config`, created on the agent's first run. As soon as you save the file, the
agent uses the new values without a restart.

```ini
# Pointer speed. 1.0 = default, 1.5 = farther, 0.7 = slower.
speed = 1.0
# Scroll speed.
scroll = 1.0
# Scroll direction. true = like a Mac trackpad (natural), false = reversed.
natural = true
# One-finger hold to drag: hold still for this many seconds, then move. 0 = off.
hold_drag = 0
# Tap, then immediately touch and move to drag.
tap_drag = false
```

Command-line options override the file:

| Option | Meaning |
|---|---|
| `--speed <number>` | pointer speed |
| `--scroll <number>` | scroll speed |
| `--natural`, `--no-natural` | scroll direction |
| `--new-code` | create a new pairing code |
| `--port <port>` | UDP port for Wi-Fi, default 50505 |
| `--buffer <ms>` | fixed Wi-Fi smoothing buffer. 0 = off. Default: automatic. |
| `--no-ble` | don't look for the phone over Bluetooth |
| `--no-aoa` | don't switch a plugged-in phone to cable mode |
| `--stats` | print connection quality every 2 seconds |
| `--dry-run -v` | don't move the pointer, just print actions |

## Troubleshooting

**The pointer doesn't move at all.** The terminal app doesn't have the Accessibility permission yet, or the agent
wasn't restarted after the permission was granted.

**Movement stutters over Wi-Fi.** Run `usap-agent --stats` and look at `telat p90` (90th percentile lateness).
If it's often above 30 ms, the problem is the Wi-Fi, not Usap. Common causes:

- The Mac is on a crowded 2.4 GHz network. Switch to 5 GHz if your router offers it.
- AirDrop, Handoff and Sidecar share the Wi-Fi radio and can cause pauses of hundreds of milliseconds.

The agent already has an adaptive buffer that evens out packets arriving in bursts. If Wi-Fi still stutters, use
the USB cable or Bluetooth.

**Touches suddenly stop, or `--stats` shows "dipotong Android" (cut off by Android).** The phone's system took
over the touch for one of its own gestures. Apps can't fully prevent this.

- Usap already protects the middle of the left and right screen edges from the back gesture. Avoid starting a
  swipe right at the edge.
- Many phones have built-in three-finger gestures, such as screenshot or split screen, that clash with Usap's
  three-finger gestures. Turn them off in the phone's settings, usually under Gestures or Special features.

**The agent doesn't find the phone over Bluetooth.** Make sure the terminal app has the Bluetooth permission in
System Settings → Privacy & Security → Bluetooth, and that the Usap screen is open on the phone after tapping
*Sambungkan lewat Bluetooth*.

**The phone can't transfer files while plugged in.** It's in Usap's cable mode. Unplug it, or run the agent with
`--no-aoa`.

## Security and privacy

- Wi-Fi and Bluetooth require the agent's 6-digit code. Five wrong codes within a minute lock out new
  connections for 60 seconds.
- The USB cable mode doesn't ask for a code, because the phone is physically plugged into the Mac.
- Packets are not encrypted. They only contain finger positions, no text or screen content, but still use Wi-Fi
  only on networks you trust.
- Usap has no server and sends nothing to the internet. The Android app does request the `INTERNET` permission,
  but only because Android requires it for local network sockets (the Wi-Fi connection).

## How it works

```
Phone (Kotlin)                                 Mac (Rust)
PadView: finger positions in mm         ──►    sources: UDP, TCP (adb), BLE, USB (AOA)
         + touch time on the phone               │
                                                 ▼
                                              jitter.rs: evens out packets that arrive in bursts
                                                 │
                                                 ▼
                                              engine.rs: gesture engine (pure, unit tested)
                                                 │
                                                 ▼
                                              mac.rs: CGEvent (pointer, clicks, scroll, shortcuts)
haptics, battery                        ◄──   replies: HAPTIC, WELCOME
```

- **The phone sends raw positions in millimetres**, not pixels, so it feels the same on any phone. All gesture
  decisions live in `agent/src/engine.rs`, pure Rust without macOS APIs, so it can be tested.
- **The network smoother** (`agent/src/jitter.rs`) uses the phone's touch timestamps to replay packets with the
  right spacing, through an adaptive buffer (4–45 ms) based on recent lateness.
- **Scrolling** is sent as continuous scroll events with phases (began, changed, ended) and momentum phases,
  like a real trackpad.
- **Bluetooth**: the phone acts as a GATT peripheral and the agent connects as a central (`btleplug`). Several
  packets are batched into one notification.
- **USB cable**: the agent uses `libusb` (through `rusb`) to switch the phone to Android Open Accessory mode, then
  reads packets with bulk transfers.

The packet format is documented in `agent/src/proto.rs`.

## Building from source

### Agent

```sh
cd agent
cargo test             # gesture engine and protocol tests
cargo build --release  # produces target/release/usap-agent
```

`libusb` is compiled into the binary, so Homebrew isn't needed.

### Android app

Choose one:

- **Android Studio**: open the `android/` folder and Run.
- **Command line**: `./build-apk.sh`. Needs JDK 17 and Android SDK 34 (`ANDROID_HOME`, or `sdk.dir` in
  `android/local.properties`).
- **On another machine over SSH and Docker**: `USAP_BUILD_HOST=<host> ./build-apk.sh`. That machine needs a
  Docker image with JDK 17 and Android SDK 34 in `/opt/android-sdk`; set its name with `USAP_BUILDER_IMAGE`. You
  can keep these settings in `build.local.env`, which git ignores.

So that a new APK installs over the old one, builds always use the same debug key. For local builds that is
Android Studio's default debug key. To use another key, put `usap.keystore=<path>` in `android/local.properties`.

## Project layout

```
agent/                 macOS agent (Rust)
  src/engine.rs        gesture engine
  src/jitter.rs        network smoother
  src/mac.rs           macOS output (CoreGraphics)
  src/proto.rs         packet format
  src/ble.rs           Bluetooth connection
  src/aoa.rs           USB cable connection (Android Open Accessory)
  src/main.rs          connections, code, settings, statistics
android/               Android app (Kotlin)
  .../PadView.kt       trackpad screen
  .../Link.kt          Wi-Fi and USB debugging connections
  .../BleLink.kt       Bluetooth connection
  .../AoaLink.kt       USB cable connection
build-apk.sh           APK build
```

## Roadmap

- A Windows agent.
- An optional four-finger gesture set for phones that reserve three-finger gestures for themselves.
- Ready-made downloads (agent and APK) on the Releases page.

## License

[MIT](LICENSE)
