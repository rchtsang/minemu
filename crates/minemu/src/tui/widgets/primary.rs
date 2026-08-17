use capstone::prelude::*;
use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::{MemRegion, PhysicalAddress, PhysicalRange, VirtualAddress};
use minemu_runtime::{ExecutionInspection, RuntimeInspection, RuntimeInspectionRequest};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::tui::{
    action::Action,
    event::AppEvent,
    input::Motion,
    types::{DialogMessage, InputMode, PrimarySubview, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
};

use super::pane_block;

pub struct PrimaryWidget {
    subview: PrimarySubview,
    memory_range: PhysicalRange,
    memory: Vec<u8>,
    execution: Option<ExecutionInspection>,
    disassembly_address: Option<VirtualAddress>,
    active: bool,
}

impl Default for PrimaryWidget {
    fn default() -> Self {
        Self {
            subview: PrimarySubview::Memory,
            memory_range: PhysicalRange::new(MemRegion::Ram.base(), 256)
                .expect("fixed RAM inspection range"),
            memory: Vec::new(),
            execution: None,
            disassembly_address: None,
            active: false,
        }
    }
}

impl PrimaryWidget {
    fn refresh(&self) -> Vec<Action> {
        let request = match self.subview {
            PrimarySubview::Memory => RuntimeInspectionRequest::LiveMemory(self.memory_range),
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

    fn nav_memory(&mut self, motion: Motion) {
        let count = motion_count(motion) as u32;
        let step: u32 = match motion {
            Motion::Left(_) | Motion::Right(_) => 1,
            Motion::Up(_) | Motion::Down(_) => 8,
            Motion::NextItem(_) | Motion::EndItem(_) => 4,
            Motion::Top | Motion::Bottom => 0,
        };
        let min = MemRegion::Ram.base().get();
        let max = min + MemRegion::Ram.size() - self.memory_range.length();
        let current = self.memory_range.start().get();
        let address = match motion {
            Motion::Top => min,
            Motion::Bottom => max,
            Motion::Left(_) | Motion::Up(_) => current.saturating_sub(step.saturating_mul(count)),
            Motion::EndItem(_) => current / 4 * 4 + 3,
            _ => current.saturating_add(step.saturating_mul(count)),
        }
        .clamp(min, max);
        self.memory_range = PhysicalRange::new(PhysicalAddress::new(address), 256)
            .expect("navigation remains in RAM");
    }

    fn nav_disassembly(&mut self, motion: Motion) {
        let current = self
            .disassembly_address
            .or_else(|| {
                self.execution
                    .as_ref()
                    .map(|value| value.instruction_address)
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

    fn render_memory(&self) -> String {
        let mut lines = vec![
            "  address             offset                    ascii".into(),
            "                     +0 +1 +2 +3 +4 +5 +6 +7".into(),
        ];
        for (index, bytes) in self.memory.chunks(8).enumerate() {
            let address = self.memory_range.start().get() + (index * 8) as u32;
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii = bytes
                .iter()
                .map(|byte| {
                    if byte.is_ascii_graphic() || *byte == b' ' {
                        *byte as char
                    } else {
                        '.'
                    }
                })
                .collect::<String>();
            lines.push(format!("0x{address:08x}: {hex:<23}  {ascii}"));
        }
        lines.join("\n")
    }

    fn render_disassembly(&self) -> String {
        let Some(execution) = &self.execution else {
            return "execution snapshot unavailable".into();
        };
        if let Some(error) = &execution.instruction_error {
            return format!("instruction bytes unavailable: {error}");
        }
        let Ok(capstone) = capstone::Capstone::new()
            .arm()
            .mode(capstone::arch::arm::ArchMode::Arm)
            .build()
        else {
            return "failed to initialize Capstone".into();
        };
        capstone
            .disasm_all(
                &execution.instruction_bytes,
                u64::from(execution.instruction_address.get()),
            )
            .map(|instructions| {
                let mut lines = vec!["  address     offset       disasm".into()];
                lines.extend(instructions.iter().map(|instruction| {
                    let bytes = instruction
                        .bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!(
                        "0x{:08x}: {:<11}  {:<8} {}",
                        instruction.address(),
                        bytes,
                        instruction.mnemonic().unwrap_or("inv"),
                        instruction.op_str().unwrap_or("")
                    )
                }));
                lines.join("\n")
            })
            .unwrap_or_else(|error| format!("disassembly failed: {error}"))
    }

    fn goto(&mut self, value: &str) -> Vec<Action> {
        let value = value.strip_prefix("0x").unwrap_or(value);
        let Ok(address) = u32::from_str_radix(value, 16) else {
            return vec![Action::ShowMessage(DialogMessage::error(
                "goto address must be hexadecimal",
            ))];
        };
        match self.subview {
            PrimarySubview::Memory => {
                let min = MemRegion::Ram.base().get();
                let max = min + MemRegion::Ram.size() - 256;
                if !(min..=max + 255).contains(&address) {
                    return vec![Action::ShowMessage(DialogMessage::error(
                        "memory address is outside physical RAM",
                    ))];
                }
                self.memory_range = PhysicalRange::new(PhysicalAddress::new(address.min(max)), 256)
                    .expect("validated RAM range");
            }
            PrimarySubview::Disassembly => {
                self.disassembly_address = Some(VirtualAddress::new(address & !3));
            }
        }
        self.refresh()
    }
}

impl TuiWidget for PrimaryWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Primary
    }

    fn visible(&self, view: View) -> bool {
        view == View::Inspect
    }

    fn render(&self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let name = match self.subview {
            PrimarySubview::Memory => "memory",
            PrimarySubview::Disassembly => "disassembly",
        };
        let text = match self.subview {
            PrimarySubview::Memory => self.render_memory(),
            PrimarySubview::Disassembly => self.render_disassembly(),
        };
        frame.render_widget(
            Paragraph::new(text).block(pane_block(
                format!("[^p] primary ({name})"),
                context.focused == self.id(),
            )),
            area,
        );
    }

    fn handle_key(&mut self, key: KeyEvent, context: &InputContext) -> Vec<Action> {
        if context.mode != InputMode::Normal {
            return Vec::new();
        }
        match key.code {
            KeyCode::Tab => vec![Action::SetPrimary(self.subview.toggled())],
            KeyCode::Char('/') if self.subview == PrimarySubview::Memory => {
                vec![Action::SetMode(InputMode::SearchAscii)]
            }
            KeyCode::Char('\\') if self.subview == PrimarySubview::Memory => {
                vec![Action::SetMode(InputMode::SearchBytes)]
            }
            KeyCode::Char('>') => vec![Action::SetMode(InputMode::Goto)],
            _ => Vec::new(),
        }
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::LiveMemory(range, bytes)) => {
                self.memory_range = *range;
                self.memory = bytes.clone();
            }
            AppEvent::Inspection(RuntimeInspection::Execution(execution)) => {
                self.execution = Some(execution.clone());
            }
            AppEvent::Inspection(RuntimeInspection::SearchMemory(Some(address))) => {
                let max = MemRegion::Ram.base().get() + MemRegion::Ram.size() - 256;
                self.memory_range =
                    PhysicalRange::new(PhysicalAddress::new(address.get().min(max)), 256)
                        .expect("search result lies in RAM");
                return vec![
                    Action::SetPrimary(PrimarySubview::Memory),
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
            AppEvent::PrimarySelected(subview) => {
                self.subview = *subview;
                if self.active {
                    return self.refresh();
                }
            }
            AppEvent::Navigate(motion) => {
                match self.subview {
                    PrimarySubview::Memory => self.nav_memory(*motion),
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
