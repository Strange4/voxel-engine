# How to?

### Setup Nvidia nsight aftermath for crash reports.

- 00: make sure that you have the [nvidia drivers](https://wiki.debian.org/NvidiaGraphicsDrivers) installed.
- 0: make sure that your shader debug info is included (in Cargo.toml)
- 1: run `/opt/nvidia/nsight-graphics-for-linux/nsight-graphics-for-linux-2025.4.1.0/host/linux-desktop-nomad-x64/nv-aftermath-monitor --crashdump-dir ~/ --debuginfo-dir ~/ --prompt-on-crash true`. This will be run in the background and have it monitor for crashes.
- 2: run `/opt/nvidia/nsight-graphics-for-linux/nsight-graphics-for-linux-2025.4.1.0/host/linux-desktop-nomad-x64/nv-aftermath-control --debuginfo true --shader-error-reporting true --mode Global`. This will set the right settings
- 3: open the application and recreate the settings
- 4: click on the prompt that appears to open the crash dump in nsight graphics

### Use renderdoc

- 1. Make sure you run with the command environment variable set to `WAYLAND_DISPLAY= XDG_SESSION_TYPE=x11 ./qrenderdoc`
- 2. Run and use renderdoc
