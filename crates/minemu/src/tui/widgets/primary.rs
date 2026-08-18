use capstone::prelude::*;
use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::{MemRegion, PhysicalAddress, PhysicalRange, VirtualAddress};
use minemu_runtime::{ExecutionInspection, RuntimeInspection, RuntimeInspectionRequest};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::tui::{
    action::Action,
    event::AppEvent,
    input::{Motion, parse_hex_address},
    types::{DialogMessage, InputMode, PrimarySubview, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
};

use super::{pane_block, render_scrollbar};

const MEMORY_REGIONS: [MemRegion; 3] = [MemRegion::BootRom, MemRegion::SystemRom, MemRegion::Ram];
const DEFAULT_MEMORY_WINDOW: u32 = 256;

pub struct PrimaryWidget {
    subview: PrimarySubview,
    memory_range: PhysicalRange,
    memory: Vec<u8>,
    memory_cursor: PhysicalAddress,
    virtual_start: VirtualAddress,
    virtual_memory: Vec<u8>,
    virtual_cursor: VirtualAddress,
    memory_columns: usize,
    memory_window: u32,
    memory_dirty: bool,
    virtual_dirty: bool,
    execution: Option<ExecutionInspection>,
    disassembly_address: Option<VirtualAddress>,
    active: bool,
}

impl Default for PrimaryWidget {
    fn default() -> Self {
        Self {
            subview: PrimarySubview::PhysicalMemory,
            memory_range: PhysicalRange::new(MemRegion::Ram.base(), DEFAULT_MEMORY_WINDOW)
                .expect("fixed RAM inspection range"),
            memory: Vec::new(),
            memory_cursor: MemRegion::Ram.base(),
            virtual_start: VirtualAddress::new(0),
            virtual_memory: Vec::new(),
            virtual_cursor: VirtualAddress::new(0),
            memory_columns: 8,
            memory_window: DEFAULT_MEMORY_WINDOW,
            memory_dirty: false,
            virtual_dirty: false,
            execution: None,
            disassembly_address: None,
            active: false,
        }
    }
}

impl PrimaryWidget {
    fn refresh(&self) -> Vec<Action> {
        let request = match self.subview {
            PrimarySubview::PhysicalMemory => {
                RuntimeInspectionRequest::LiveMemory(self.memory_range)
            }
            PrimarySubview::VirtualMemory => RuntimeInspectionRequest::VirtualMemory {
                address: self.virtual_start,
                length: self.memory_window as usize,
            },
            PrimarySubview::Disassembly => RuntimeInspectionRequest::Execution {
                address: self.disassembly_address,
                before: usize::from(self.disassembly_address.is_none()) * 32,
                after: 224,
            },
        };
        vec![Action::RequestInspection {
            target: self.id(),
            request,
        }]
    }

    fn nav_physical_memory(&mut self, motion: Motion) {
        let count = motion_count(motion);
        let step = match motion {
            Motion::Left(_) | Motion::Right(_) => 1,
            Motion::Up(_) | Motion::Down(_) => self.memory_columns,
            Motion::NextItem(_) | Motion::EndItem(_) => 4,
            Motion::Top | Motion::Bottom => 0,
        };
        let current = memory_linear_offset(self.memory_cursor).unwrap_or(0);
        let maximum = memory_map_size().saturating_sub(1);
        let linear = match motion {
            Motion::Top => 0,
            Motion::Bottom => maximum,
            Motion::Left(_) | Motion::Up(_) => current.saturating_sub(step.saturating_mul(count)),
            Motion::EndItem(_) => current / 4 * 4 + 3,
            _ => current.saturating_add(step.saturating_mul(count)),
        }
        .min(maximum);
        self.memory_cursor = memory_address_at(linear);
        self.ensure_cursor_visible();
    }

    fn nav_virtual_memory(&mut self, motion: Motion) {
        let count = motion_count(motion) as u32;
        let step = match motion {
            Motion::Left(_) | Motion::Right(_) => 1,
            Motion::Up(_) | Motion::Down(_) => self.memory_columns as u32,
            Motion::NextItem(_) | Motion::EndItem(_) => 4,
            Motion::Top | Motion::Bottom => 0,
        };
        let current = self.virtual_cursor.get();
        let address = match motion {
            Motion::Top => 0,
            Motion::Bottom => u32::MAX,
            Motion::Left(_) | Motion::Up(_) => current.saturating_sub(step.saturating_mul(count)),
            Motion::EndItem(_) => current / 4 * 4 + 3,
            _ => current.saturating_add(step.saturating_mul(count)),
        };
        self.virtual_cursor = VirtualAddress::new(address);
        self.ensure_virtual_cursor_visible();
    }

    fn nav_disassembly(&mut self, motion: Motion) {
        let current = self
            .disassembly_address
            .or_else(|| {
                self.execution
                    .as_ref()
                    .map(|value| VirtualAddress::new(value.registers[15]))
            })
            .unwrap_or(VirtualAddress::new(0));
        let count = motion_count(motion) as u32;
        let step = match motion {
            Motion::Up(_) | Motion::Down(_) | Motion::NextItem(_) | Motion::EndItem(_) => 4,
            Motion::Left(_) | Motion::Right(_) => 1,
            Motion::Top | Motion::Bottom => 0,
        };
        let address = match motion {
            Motion::Top => 0,
            Motion::Bottom => u32::MAX - 255,
            Motion::Left(_) | Motion::Up(_) => current.get().saturating_sub(step * count),
            _ => current.get().saturating_add(step * count),
        };
        self.disassembly_address = Some(VirtualAddress::new(address & !3));
    }

    fn render_memory(&self, virtual_memory: bool) -> Text<'static> {
        let columns = self.memory_columns;
        let (address_label, start, bytes, cursor) = if virtual_memory {
            (
                "vaddr",
                self.virtual_start.get(),
                &self.virtual_memory,
                self.virtual_cursor.get(),
            )
        } else {
            (
                "paddr",
                self.memory_range.start().get(),
                &self.memory,
                self.memory_cursor.get(),
            )
        };
        let mut lines = vec![
            Line::raw(format!(
                "{:<10}{:<width$} ascii",
                address_label,
                "offset",
                width = columns * 3
            )),
            Line::raw(format!(
                "{}{offsets}",
                " ".repeat(10),
                offsets = (0..columns)
                    .map(|offset| format!("+{offset} "))
                    .collect::<String>()
            )),
        ];
        let selected = Style::default()
            .fg(Color::Black)
            .bg(Color::LightYellow)
            .add_modifier(Modifier::BOLD);
        for (row, bytes) in bytes.chunks(columns).enumerate() {
            let address = start + (row * columns) as u32;
            let mut spans = vec![Span::raw(format!("{address:08x}: "))];
            for (column, byte) in bytes.iter().enumerate() {
                let byte_address = address + column as u32;
                let style = if byte_address == cursor {
                    selected
                } else {
                    Style::default()
                };
                spans.push(Span::styled(format!("{byte:02x}"), style));
                spans.push(Span::raw(" "));
            }
            for _ in bytes.len()..columns {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::raw(" "));
            for (column, byte) in bytes.iter().enumerate() {
                let character = if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                };
                let style = if address + column as u32 == cursor {
                    selected
                } else {
                    Style::default()
                };
                spans.push(Span::styled(character.to_string(), style));
            }
            lines.push(Line::from(spans));
        }
        Text::from(lines)
    }

    fn render_disassembly(&self) -> Text<'static> {
        let Some(execution) = &self.execution else {
            return Text::raw("execution snapshot unavailable");
        };
        if let Some(error) = &execution.instruction_error {
            return Text::raw(format!("instruction bytes unavailable: {error}"));
        }
        let Ok(capstone) = capstone::Capstone::new()
            .arm()
            .mode(capstone::arch::arm::ArchMode::Arm)
            .build()
        else {
            return Text::raw("failed to initialize Capstone");
        };
        capstone
            .disasm_all(
                &execution.instruction_bytes,
                u64::from(execution.instruction_address.get()),
            )
            .map(|instructions| {
                let address_label = if execution.mmu_enabled {
                    "vaddr"
                } else {
                    "paddr"
                };
                let mut lines = vec![Line::raw(format!(
                    "  {address_label:<10} raw          disasm"
                ))];
                let pc = u64::from(execution.registers[15]);
                let selected = Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD);
                lines.extend(instructions.iter().map(|instruction| {
                    let bytes = instruction
                        .bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let address = format!("0x{:08x}:", instruction.address());
                    Line::from(vec![
                        Span::styled(
                            address,
                            if instruction.address() == pc {
                                selected
                            } else {
                                Style::default()
                            },
                        ),
                        Span::raw(format!(
                            " {:<11}  {:<8} {}",
                            bytes,
                            instruction.mnemonic().unwrap_or("inv"),
                            instruction.op_str().unwrap_or("")
                        )),
                    ])
                }));
                Text::from(lines)
            })
            .unwrap_or_else(|error| Text::raw(format!("disassembly failed: {error}")))
    }

    fn goto(&mut self, value: &str) -> Vec<Action> {
        let address = match parse_hex_address(value) {
            Ok(address) => address,
            Err(error) => return vec![Action::ShowMessage(DialogMessage::error(error))],
        };
        match self.subview {
            PrimarySubview::PhysicalMemory => {
                let address = PhysicalAddress::new(address);
                if memory_region(address).is_none() {
                    return vec![Action::ShowMessage(DialogMessage::error(
                        "address is outside Boot ROM, system ROM, and RAM",
                    ))];
                }
                self.memory_cursor = address;
                self.ensure_cursor_visible();
            }
            PrimarySubview::VirtualMemory => {
                self.virtual_cursor = VirtualAddress::new(address);
                self.ensure_virtual_cursor_visible();
            }
            PrimarySubview::Disassembly => {
                self.disassembly_address = Some(VirtualAddress::new(address & !3));
            }
        }
        self.refresh()
    }

    fn ensure_cursor_visible(&mut self) {
        let region = memory_region(self.memory_cursor).expect("cursor remains in mapped memory");
        let base = region.base().get();
        let window = self.memory_window.min(region.size());
        let last_start = base + region.size() - window;
        let cursor = self.memory_cursor.get();
        let current_start = self.memory_range.start().get();
        let current_end = current_start + self.memory_range.length();
        let same_region = memory_region(self.memory_range.start()) == Some(region);
        let start = if same_region && (current_start..current_end).contains(&cursor) {
            current_start.min(last_start)
        } else if cursor < current_start || !same_region {
            (cursor - (cursor - base) % self.memory_columns as u32).min(last_start)
        } else {
            cursor
                .saturating_sub(window - self.memory_columns as u32)
                .min(last_start)
        };
        self.memory_range = PhysicalRange::new(PhysicalAddress::new(start), window)
            .expect("cursor window remains in mapped memory");
    }

    fn ensure_virtual_cursor_visible(&mut self) {
        let window = self.memory_window;
        let last_start = u32::MAX - window.saturating_sub(1);
        let cursor = self.virtual_cursor.get();
        let current_start = self.virtual_start.get();
        let current_end = u64::from(current_start) + u64::from(window);
        let start = if (u64::from(current_start)..current_end).contains(&u64::from(cursor)) {
            current_start.min(last_start)
        } else if cursor < current_start {
            (cursor - cursor % self.memory_columns as u32).min(last_start)
        } else {
            cursor
                .saturating_sub(window - self.memory_columns as u32)
                .min(last_start)
        };
        self.virtual_start = VirtualAddress::new(start);
    }

    fn update_memory_geometry(&mut self, area: Rect) {
        let columns = if area.width >= 46 { 8 } else { 4 };
        let rows = area.height.saturating_sub(4).max(1);
        let window = u32::from(rows) * columns as u32;
        if self.memory_columns == columns && self.memory_window == window {
            return;
        }
        self.memory_columns = columns;
        self.memory_window = window;
        self.ensure_cursor_visible();
        self.ensure_virtual_cursor_visible();
        self.memory.clear();
        self.virtual_memory.clear();
        self.memory_dirty = true;
        self.virtual_dirty = true;
    }
}

impl TuiWidget for PrimaryWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Primary
    }

    fn visible(&self, view: View) -> bool {
        view == View::Inspect
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let name = match self.subview {
            PrimarySubview::PhysicalMemory => "pmem",
            PrimarySubview::VirtualMemory => "vmem",
            PrimarySubview::Disassembly => "disasm",
        };
        match self.subview {
            PrimarySubview::PhysicalMemory | PrimarySubview::VirtualMemory => {
                self.update_memory_geometry(area);
                let virtual_memory = self.subview == PrimarySubview::VirtualMemory;
                frame.render_widget(
                    Paragraph::new(self.render_memory(virtual_memory)).block(pane_block(
                        format!("[^p] primary ({name})"),
                        context.focused == self.id(),
                        area.width,
                    )),
                    area,
                );
                render_scrollbar(
                    frame,
                    area,
                    if virtual_memory {
                        u32::MAX as usize + 1
                    } else {
                        memory_map_size()
                    },
                    self.memory_window as usize,
                    if virtual_memory {
                        self.virtual_start.get() as usize
                    } else {
                        memory_linear_offset(self.memory_range.start()).unwrap_or(0)
                    },
                );
            }
            PrimarySubview::Disassembly => {
                frame.render_widget(
                    Paragraph::new(self.render_disassembly()).block(pane_block(
                        format!("[^p] primary ({name})"),
                        context.focused == self.id(),
                        area.width,
                    )),
                    area,
                );
                let position = self
                    .execution
                    .as_ref()
                    .map(|execution| execution.instruction_address.get() as usize / 4)
                    .unwrap_or(0);
                render_scrollbar(
                    frame,
                    area,
                    u32::MAX as usize / 4,
                    usize::from(area.height.saturating_sub(2)),
                    position,
                );
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, context: &InputContext) -> Vec<Action> {
        if context.mode != InputMode::Normal {
            return Vec::new();
        }
        match key.code {
            KeyCode::Tab => vec![Action::SetPrimary(self.subview.next())],
            KeyCode::Char('/') if self.subview == PrimarySubview::PhysicalMemory => {
                vec![Action::SetMode(InputMode::SearchAscii)]
            }
            KeyCode::Char('\\') if self.subview == PrimarySubview::PhysicalMemory => {
                vec![Action::SetMode(InputMode::SearchBytes)]
            }
            KeyCode::Char('>') => vec![Action::SetMode(InputMode::Goto)],
            _ => Vec::new(),
        }
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::LiveMemory(range, bytes)) => {
                if *range == self.memory_range {
                    self.memory = bytes.clone();
                    self.memory_dirty = false;
                }
            }
            AppEvent::Inspection(RuntimeInspection::VirtualMemory(address, bytes)) => {
                if *address == self.virtual_start && bytes.len() == self.memory_window as usize {
                    self.virtual_memory = bytes.clone();
                    self.virtual_dirty = false;
                }
            }
            AppEvent::Inspection(RuntimeInspection::Execution(execution)) => {
                self.execution = Some(execution.clone());
            }
            AppEvent::Inspection(RuntimeInspection::SearchMemory(Some(address))) => {
                self.memory_cursor = *address;
                self.ensure_cursor_visible();
                return vec![
                    Action::SetPrimary(PrimarySubview::PhysicalMemory),
                    Action::ShowMessage(DialogMessage::info(format!(
                        "match at 0x{:08x}",
                        address.get()
                    ))),
                    Action::Refresh(self.id()),
                ];
            }
            AppEvent::Inspection(RuntimeInspection::SearchMemory(None)) => {
                return vec![Action::ShowMessage(DialogMessage::info(
                    "pattern not found in physical RAM",
                ))];
            }
            AppEvent::ViewChanged(view) => {
                self.active = *view == View::Inspect;
                if self.active {
                    return vec![Action::Refresh(self.id())];
                }
            }
            AppEvent::Refresh if self.active => return self.refresh(),
            AppEvent::Pulse if self.active => match self.subview {
                PrimarySubview::PhysicalMemory if self.memory_dirty => {
                    self.memory_dirty = false;
                    return self.refresh();
                }
                PrimarySubview::VirtualMemory if self.virtual_dirty => {
                    self.virtual_dirty = false;
                    return self.refresh();
                }
                _ => {}
            },
            AppEvent::PrimarySelected(subview) => {
                self.subview = *subview;
                if self.active {
                    return self.refresh();
                }
            }
            AppEvent::Navigate(motion) => {
                match self.subview {
                    PrimarySubview::PhysicalMemory => self.nav_physical_memory(*motion),
                    PrimarySubview::VirtualMemory => self.nav_virtual_memory(*motion),
                    PrimarySubview::Disassembly => self.nav_disassembly(*motion),
                }
                return self.refresh();
            }
            AppEvent::Goto(value) => return self.goto(value),
            _ => {}
        }
        Vec::new()
    }
}

fn memory_map_size() -> usize {
    MEMORY_REGIONS
        .iter()
        .map(|region| region.size() as usize)
        .sum()
}

fn memory_region(address: PhysicalAddress) -> Option<MemRegion> {
    MEMORY_REGIONS
        .into_iter()
        .find(|region| region.range().contains_address(address))
}

fn memory_linear_offset(address: PhysicalAddress) -> Option<usize> {
    let mut offset = 0usize;
    for region in MEMORY_REGIONS {
        if region.range().contains_address(address) {
            return Some(offset + (address.get() - region.base().get()) as usize);
        }
        offset += region.size() as usize;
    }
    None
}

fn memory_address_at(mut offset: usize) -> PhysicalAddress {
    for region in MEMORY_REGIONS {
        if offset < region.size() as usize {
            return PhysicalAddress::new(region.base().get() + offset as u32);
        }
        offset -= region.size() as usize;
    }
    let region = MemRegion::Ram;
    PhysicalAddress::new(region.base().get() + region.size() - 1)
}

fn motion_count(motion: Motion) -> usize {
    match motion {
        Motion::Left(count)
        | Motion::Down(count)
        | Motion::Up(count)
        | Motion::Right(count)
        | Motion::NextItem(count)
        | Motion::EndItem(count) => count,
        Motion::Top | Motion::Bottom => 1,
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{MemRegion, PhysicalAddress, VirtualAddress};
    use minemu_runtime::ExecutionInspection;
    use ratatui::layout::Rect;

    use super::{PrimaryWidget, memory_address_at, memory_linear_offset};
    use crate::tui::input::Motion;

    #[test]
    fn linear_memory_map_includes_both_roms_and_ram() {
        assert_eq!(memory_linear_offset(MemRegion::BootRom.base()), Some(0));
        assert_eq!(
            memory_linear_offset(MemRegion::SystemRom.base()),
            Some(MemRegion::BootRom.size() as usize)
        );
        assert_eq!(
            memory_address_at(MemRegion::BootRom.size() as usize),
            MemRegion::SystemRom.base()
        );
    }

    #[test]
    fn cursor_skips_the_gap_between_rom_regions() {
        let mut widget = PrimaryWidget {
            memory_cursor: PhysicalAddress::new(
                MemRegion::BootRom.base().get() + MemRegion::BootRom.size() - 1,
            ),
            ..PrimaryWidget::default()
        };
        widget.ensure_cursor_visible();
        widget.nav_physical_memory(Motion::Right(1));
        assert_eq!(widget.memory_cursor, MemRegion::SystemRom.base());
        assert_eq!(
            Option::<MemRegion>::from(widget.memory_range.start()),
            Some(MemRegion::SystemRom)
        );
    }

    #[test]
    fn memory_window_fills_visible_data_rows() {
        let mut widget = PrimaryWidget::default();
        widget.update_memory_geometry(Rect::new(0, 0, 60, 20));
        assert_eq!(widget.memory_columns, 8);
        assert_eq!(widget.memory_range.length(), 16 * 8);

        widget.update_memory_geometry(Rect::new(0, 0, 40, 20));
        assert_eq!(widget.memory_columns, 4);
        assert_eq!(widget.memory_range.length(), 16 * 4);
    }

    #[test]
    fn disassembly_address_label_follows_mmu_state() {
        let mut widget = PrimaryWidget {
            execution: Some(ExecutionInspection {
                registers: [0; 16],
                cpsr: 0,
                spsr: 0,
                mmu_enabled: false,
                instruction_address: VirtualAddress::new(0),
                instruction_bytes: vec![0, 0, 0xa0, 0xe1],
                instruction_error: None,
            }),
            ..PrimaryWidget::default()
        };
        let heading = widget.render_disassembly().lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(heading.contains("paddr"));

        widget.execution.as_mut().unwrap().mmu_enabled = true;
        let heading = widget.render_disassembly().lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(heading.contains("vaddr"));
    }
}
