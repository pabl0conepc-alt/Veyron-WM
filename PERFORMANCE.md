# Performance notes

No Veyron benchmarks or measurements exist yet.

## Target development machine

- Intel Core i3-2370M, 2 cores / 4 threads.
- 4 GiB RAM; about 1.5 GiB was available at inspection.
- Intel graphics PCI ID `8086:0116` (HD Graphics 3000 identification inferred from the PCI ID; actual Mesa driver capabilities not yet read).

## Measurement plan

Before setting targets, record idle and client-load RSS, CPU usage, output refresh rate, render/frame timing, and input-to-action timing. Use serial, low-debug-info compilation to avoid memory pressure. Do not include compiler/build memory in compositor runtime figures. Keep a repeatable client workload and record kernel, driver, output mode, and build profile with every result.
