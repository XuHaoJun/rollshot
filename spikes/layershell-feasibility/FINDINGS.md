# iced_layershell Feasibility Spike — Findings

## Environment
- KDE Plasma version: not installed on this machine (headless VM)
- KWin version: not installed on this machine (headless VM)
- Mesa / GPU: VMware SVGA II Adapter (Mesa 25.2.8, libgl1-mesa-dri)
- Session: XDG_SESSION_TYPE=tty (not Wayland — runtime tests require a Plasma Wayland session)
- iced_layershell dep form: crates.io 0.18 (iced 0.14 + iced_layershell 0.18, resolved from crates.io)

## Risk results (filled per task)
| Risk | Task | Result | Notes |
|------|------|--------|-------|
| R6 transparency/layer/Esc | 2 | compiles | Transparent fullscreen overlay compiles. `Color::TRANSPARENT` style, `Layer::Overlay`, all-anchors sizing, `KeyboardInteractivity::Exclusive`. Esc via `keyboard::Key::Named(keyboard::key::Named::Escape)` confirmed at compile time. Runtime observation requires KDE 6 hardware. |
| R1 wgpu coexistence | 3 | | |
| R2 focus/clipboard | 3 | | |
| R6 controls/text | 4 | | |
| R6 input region / scroll passthrough | 5 | | |
| R6 preview refresh | 6 | | |
| R3 self-capture | 7 | | |
| R4 fractional scaling | 7 | | |
| R5 output match | 7 | | |
| R7 multi-monitor | 8 | | |

## Decision
<filled in Task 9>
