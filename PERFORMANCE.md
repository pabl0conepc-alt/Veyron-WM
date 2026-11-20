# Performance engineering notes

## Target machine

- Intel Core i3-2370M, 2 cores / 4 threads, 4 GiB RAM.
- Display PCI ID `8086:0116` suggests Intel HD Graphics 3000; active Mesa/EGL
  driver and acceleration path still need a direct runtime query.

## Current implementation

- The headless calloop loop waits on the Wayland display fd and control channel;
  it retains a 50 ms bound to retry blocked client flushes.
- The nested loop now waits on calloop readiness from Wayland, IPC and Winit
  sources rather than waking on a fixed 16 ms timer. The headless loop keeps a
  50 ms retry bound for blocked socket flushes.
- The nested renderer now tracks scene damage with Smithay and passes damaged
  regions to EGL. When the backend reports no usable buffer age, Smithay
  requests a complete redraw for correctness.
- GitHub CI has built and tested the workspace, but it is not a runtime
  performance measurement for the target laptop.

## Measurement plan

Use a repeatable nested client workload and record the commit, Rust profile,
kernel, renderer/driver, output size and refresh rate. Measure idle and load RSS,
CPU, startup, frame-time distribution, input-to-action latency, layout cost and
workspace-switch cost. Report raw samples and tool commands; set targets only
after establishing a baseline. Do not include compilation memory in compositor
runtime measurements.
