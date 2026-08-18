# overall behaviors

- errors should generally not crash the tui, but be displayed with red text in the dialog pane
- emulation starts paused in TUI mode and requires an explicit start action

# commands and controls

| command                  | description |
| :---                     | :--- |
| :q / :quit               | quit tui |
| :? / :help               | open help window |
| :s                       | toggle emulation start/stop |
| :start                   | start emulation |
| :stop                    | stop emulation  |
| :reset                   | reset emulator (emulated power cycle) |
| :v [opt] / :view [opt]   | change view (options: [r]untime, [i]nspect; no option toggles) |
| :set [key] [val]         | set configuration values |
| :g / :goto [loc]         | goto location if applicable ([loc] depends on pane) |

Configuration Values:
| key       | values        | description |
| :---      | :---          | :---        |
| uart      | 0/1           | select uart source |
| primary   | mem/disasm    | select primary subview ([mem]ory, [disasm]) |
| secondary | reg/peri/pend | select secondary subview ([reg]isters, [peri]pherals, [pend]ing exceptions |


Leader Commands:
- Runtime View: (`<space>`+r)
- Inspect View: (`<space>`+i)
- Start/Stop Emulation (`<space>`+s)

Misc Controls:
- Resize Pane: `Ctrl` + `Left Click` + Drag


# runtime view (default)

```
 [ runtime ]   inspect                                                                             
                                                                                                   
┌─ [^c] console ────────────────────────────────────────────────┐┌─ [^e] events ──────────────────┐
│                                                             ▲ ││                              ▲ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              ▼ │
│                                                             │ │└────────────────────────────────┘
│                                                             │ │┌─ [^d] dialog ──────────────────┐
│                                                             │ ││ <command output/ui messages>   │
│                                                             │ ││                              ▲ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             ▼ ││                              ▼ │
└───────────────────────────────────────────────────────────────┘└────────────────────────────────┘
 <input>                                                                                           
 <hints> Leader: <space> Help: ?                                        ticks : 0x0000000000000000 
```

Notes:
- the focused pane should be bolded and have a brighter foreground.
  unfocused panes should have a dimmer foreground, but still be clearly visible.
- the `<input>` bar should have a different background color.
  it should show the user's key inputs and be cleared when a command or action is detected and taken.
- the `<hints>` bar should show a list of hints depending on the active pane.
- the hints and tick sections are separated by a vertical border
- scrollable panes show a vertical scrollbar
  - scrollbar tracks are inset and do not overwrite pane corners

Panes:
- console
  - Normal Mode hints:
    - Insert: i
    - Scroll: `[#]` {jk}
    - Leader: `<space>`
    - Help: ?
  - Insert Mode hints:
    - Normal Mode: `<esc>`
- events (Normal Mode only)
  - hints:
    - Scroll: `[#]` {jk}
    - Goto Current: G
- dialog (Normal Mode only)
  - hints:
    - Scroll: `[#]` {jk}
    - Goto Current: G

# inspect view

```
   runtime   [ inspect ]                                                                           
                                                                                                   
┌─ [^p] primary (memory) ───────────────────────────────────────┐┌─ [^s] secondary (registers) ───┐
│   address            offset              ascii              ▲ ││        hex         decimal   ▲ │
│             +0 +1 +2 +3 +4 +5 +6 +7                         │ ││ pc  0x4000355c    1073755484 │ │
│ 0x40001000: ff ff ff ff ff ff ff ff     ........            │ ││ lr  0x40000424    1073742884 │ │
│ 0x40001008: ff ff ff ff ff ff ff ff     ........            │ ││ r0  0x00000000             0 │ │
│ 0x40001010: ff ff ff ff ff ff ff ff     ........            │ ││ r1  0x00000010            16 │ │
│ 0x40001018: ff ff ff ff ff ff ff ff     ........            │ ││ r2                           │ │
│ 0x40001020: ff ff ff ff ff ff ff ff     ........            │ ││ r3                           │ │
│     ...                ...                ...               │ ││ r4                           │ │
│                                                             │ ││ r5                           ▼ │
│                                                             │ │└────────────────────────────────┘
│   address     offset        disasm                          │ │┌─ [^d] dialog ──────────────────┐
│             +0 +1 +2 +3                                     │ ││ <command output/ui messages> ▲ │
│ 0x40001000: ff ff ff ff    inv                              │ ││                              │ │
│ 0x40001004: ff ff ff ff    inv                              │ ││                              │ │
│ 0x40001008: ff ff ff ff    inv                              │ ││                              │ │
│ 0x4000100c: ff ff ff ff    inv                              │ ││                              │ │
│ 0x40001010: ff ff ff ff    inv                              │ ││                              │ │
│     ...         ...          ...                            │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             │ ││                              │ │
│                                                             ▼ ││                              ▼ │
└───────────────────────────────────────────────────────────────┘└────────────────────────────────┘
 <input>                                                                                           
 <hints> Leader: <space>  Help: ?                                       ticks : 0x0000000000000000 
```


Panes:
- primary (see subviews)
  - Normal Mode:
    - Move: `[#]` {hjkl}
    - Switch Subview: `<tab>`
- secondary (see subviews)
  - Normal Mode:
    - Move: `[#]` {hjkl}
    - Switch Subview: `<tab>`
- dialog
  - hints:
    - Scroll: `[#]` {jk}
    - Goto Current: G

Subviews:
- only the selected primary and secondary subviews are rendered; the stacked
  memory/disassembly rows above illustrate both primary formats rather than two
  simultaneously visible panes
- memory (primary)
  - covers Boot ROM, system ROM, and RAM while skipping unmapped/MMIO gaps
  - movement selects bytes with a cursor and scrolls the memory window
  - shows eight bytes per row when wide enough and four bytes otherwise
  - addresses omit the `0x` prefix to preserve space for byte columns
  - Normal Mode:
    - Search ASCII: /[pattern]
    - Search Bytes: \\[pattern]
    - Goto Address: >`[addr]`
- disassembly (primary)
  - Normal Mode:
    - Goto Address: >`[addr]`
- registers (secondary)
  - Normal Mode:
    - Goto Register: >`[reg]`
- peripherals (secondary)
- pending (secondary)

The dialog wraps messages and prefixes each new message with `>`. Help is shown
as a white command/description table in a green-bordered popup rather than
appended to dialog history. Narrow register panes omit
decimal conversion and do not scroll beyond the last full page of registers.

Memory searches scan all physical RAM. ASCII search uses UTF-8 input and byte
search uses whitespace-separated hexadecimal pairs such as `\de ad be ef`.

# asciiflow

[asciiflow src](https://asciiflow.com/#/share/eJztmMtum0AUhl%2FlaNZRO2CIKbLcRTd9h0AlNIyTUblpBixbUaQq6y6ysKI%2BR9d9Gj9JhxpsIMbF5pK04gjLeDjMNxfzz%2Fzco8DxKTKDxPOukOesKUcmurfQknLBwsBCpnploZX8%2FqBhebZOSwxDnsV0FcsfFoIb4EkQM5%2BCDQAsEBElMXQZlhV0Wl8tZbv5vt18g5svxAYSBiL0KMiCN3Y87ZtJbaBLGsTi4lY%2B%2Fen1Y6uB2z7%2FlPzHv1aT5XVBlPc3Iu7yRuJIHIkv855%2FdU3c9K9%2BP7psb6ajoQ1hEkdJ%2FLo6epi3GQl93wncrFnvEwY%2BFcK5pWK%2Byxz0fzLq9kgciW%2BG2J1u72pqUE3OG0Dg%2B109BlujwMxFfN5qjk5ESpndMbn%2Fn8Nn6kUmfLy0qpiRrwJMwCtcieHc12CU3K%2BC9K65X7U7p%2FQdRb%2Fq23J%2F4Id8Da%2F9gB09Do6V28DpLRMx5Web1sMey3FdLrdDxdEIFwtBK68dHEEYKxe99Kp3dLW%2F6lLCfMeDU3se%2BURgBbAKeAJYA6wDvgY8rZ2mgqpHRN6%2B0uRDNdF1kl5U8HQy1XXN0KC8juzSFPkxYbE4fqTxLosaosdzItZUbU%2FUVKOeaLQicgwFESmPXA1RaddHrhyISomoXNcRW%2FZRrZ3t6n4gJ6ot%2BzhpTMxqqyY1KioStbOIl0aRqJ%2FK%2B8e9alGxylLlMuEIv0F7e%2FWqRVU7axTP8Kp1mgbpW%2BNlY97pvJJu74laj8RGKjoAkQxNrOp2f8Q0impVUq4jytYN8dIYiYW80atevnr8r171044lTJiJyCFUQs%2Bxr6e8KnpAD78B%2BAy4Og%3D%3D))
