use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::{MmuInspection, PeripheralsInspection};
use minemu_runtime::{ExecutionInspection, RuntimeInspection, RuntimeInspectionRequest};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::tui::{
    action::Action,
    event::AppEvent,
    input::Motion,
    types::{DialogMessage, InputMode, SecondarySubview, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
};

use super::{pane_block, render_scrollbar};

const REGISTER_COUNT: usize = 18;

pub struct SecondaryWidget {
    subview: SecondarySubview,
    execution: Option<ExecutionInspection>,
    peripherals: Option<PeripheralsInspection>,
    mmu: Option<MmuInspection>,
    row: usize,
    max_row: usize,
    active: bool,
}

impl Default for SecondaryWidget {
    fn default() -> Self {
        Self {
            subview: SecondarySubview::Registers,
            execution: None,
            peripherals: None,
            mmu: None,
            row: 0,
            max_row: REGISTER_COUNT - 1,
            active: false,
        }
    }
}

impl SecondaryWidget {
    fn refresh(&self) -> Vec<Action> {
        match self.subview {
            SecondarySubview::Registers => vec![Action::RequestInspection {
                target: self.id(),
                request: RuntimeInspectionRequest::Execution {
                    address: None,
                    before: 0,
                    after: 4,
                },
            }],
            SecondarySubview::Peripherals => vec![Action::RequestInspection {
                target: self.id(),
                request: RuntimeInspectionRequest::Machine(
                    minemu_platform::InspectionRequest::Peripherals,
                ),
            }],
            SecondarySubview::Pending => vec![
                Action::RequestInspection {
                    target: self.id(),
                    request: RuntimeInspectionRequest::Machine(
                        minemu_platform::InspectionRequest::Mmu,
                    ),
                },
                Action::RequestInspection {
                    target: self.id(),
                    request: RuntimeInspectionRequest::Machine(
                        minemu_platform::InspectionRequest::Peripherals,
                    ),
                },
            ],
        }
    }

    fn registers(&self, show_decimal: bool) -> String {
        let Some(execution) = &self.execution else {
            return "register snapshot unavailable".into();
        };
        let mut values = vec![
            ("pc".to_string(), execution.registers[15]),
            ("lr".to_string(), execution.registers[14]),
            ("sp".to_string(), execution.registers[13]),
        ];
        values.extend((0..13).map(|index| (format!("r{index}"), execution.registers[index])));
        values.push(("cpsr".into(), execution.cpsr));
        values.push(("spsr".into(), execution.spsr));
        let mut lines = vec![if show_decimal {
            "       hex          decimal".into()
        } else {
            "       hex".into()
        }];
        lines.extend(values.into_iter().skip(self.row).map(|(name, value)| {
            if show_decimal {
                format!("{name:<5} 0x{value:08x}  {value:>10}")
            } else {
                format!("{name:<5} 0x{value:08x}")
            }
        }));
        lines.join("\n")
    }

    fn peripherals(&self) -> String {
        self.peripherals.as_ref().map_or_else(
            || "peripheral snapshot unavailable".into(),
            |p| {
                format!(
                    "UART0 status 0x{:08x} control 0x{:08x}\nUART1 status 0x{:08x} control 0x{:08x}\nSysTick period {} control 0x{:08x} status 0x{:08x}\nBlock lba {} sectors {} dma 0x{:08x}\nBlock status 0x{:08x} error {}\nRNG state 0x{:08x}",
                    p.uart0.status,
                    p.uart0.control,
                    p.uart1.status,
                    p.uart1.control,
                    p.systick.period,
                    p.systick.control,
                    p.systick.status,
                    p.block.lba,
                    p.block.sector_count,
                    p.block.dma_address,
                    p.block.status,
                    p.block.error,
                    p.rng.state,
                )
            },
        )
    }

    fn pending(&self) -> String {
        let irq = self.peripherals.as_ref().map(|p| {
            format!(
                "IRQ pending 0x{:08x}\nIRQ enabled 0x{:08x}\nIRQ claim {:?}",
                p.interrupts.pending, p.interrupts.enabled, p.interrupts.claim
            )
        });
        let mmu = self.mmu.map(|m| {
            format!(
                "MMU enabled {}\nlast fault address {:?}\nlast fault status {:?}",
                m.enabled, m.last_fault_address, m.last_fault_status
            )
        });
        [irq, mmu]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn goto_register(&mut self, value: &str) -> Vec<Action> {
        self.row = match value.to_ascii_lowercase().as_str() {
            "pc" => 0,
            "lr" => 1,
            "sp" => 2,
            register if register.starts_with('r') => {
                let Ok(value) = register[1..].parse::<usize>() else {
                    return vec![Action::ShowMessage(DialogMessage::error(
                        "unknown register",
                    ))];
                };
                if value >= 13 {
                    return vec![Action::ShowMessage(DialogMessage::error(
                        "unknown register",
                    ))];
                }
                value + 3
            }
            "cpsr" => 16,
            "spsr" => 17,
            _ => {
                return vec![Action::ShowMessage(DialogMessage::error(
                    "unknown register",
                ))];
            }
        };
        Vec::new()
    }
}

impl TuiWidget for SecondaryWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Secondary
    }

    fn visible(&self, view: View) -> bool {
        view == View::Inspect
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let viewport = usize::from(area.height.saturating_sub(3)).max(1);
        let (name, text, content_length, sticky_header) = match self.subview {
            SecondarySubview::Registers => (
                "registers",
                self.registers(area.width >= 34),
                REGISTER_COUNT,
                true,
            ),
            SecondarySubview::Peripherals => {
                let text = self.peripherals();
                let length = text.lines().count();
                ("peripherals", text, length, false)
            }
            SecondarySubview::Pending => {
                let text = self.pending();
                let length = text.lines().count();
                ("pending", text, length, false)
            }
        };
        self.max_row = content_length.saturating_sub(viewport);
        self.row = self.row.min(self.max_row);
        let scroll = if sticky_header { 0 } else { self.row as u16 };
        frame.render_widget(
            Paragraph::new(text).scroll((scroll, 0)).block(pane_block(
                format!("[^s] secondary ({name})"),
                context.focused == self.id(),
            )),
            area,
        );
        render_scrollbar(frame, area, content_length, viewport, self.row);
    }

    fn handle_key(&mut self, key: KeyEvent, context: &InputContext) -> Vec<Action> {
        if context.mode != InputMode::Normal {
            return Vec::new();
        }
        match key.code {
            KeyCode::Tab => vec![Action::SetSecondary(self.subview.next())],
            KeyCode::Char('>') if self.subview == SecondarySubview::Registers => {
                vec![Action::SetMode(InputMode::Goto)]
            }
            _ => Vec::new(),
        }
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::Execution(execution)) => {
                self.execution = Some(execution.clone());
            }
            AppEvent::Inspection(RuntimeInspection::Peripherals(peripherals)) => {
                self.peripherals = Some(peripherals.clone());
            }
            AppEvent::Inspection(RuntimeInspection::Mmu(mmu)) => self.mmu = Some(*mmu),
            AppEvent::ViewChanged(view) => {
                self.active = *view == View::Inspect;
                if self.active {
                    return vec![Action::Refresh(self.id())];
                }
            }
            AppEvent::Refresh if self.active => return self.refresh(),
            AppEvent::SecondarySelected(subview) => {
                self.subview = *subview;
                self.row = 0;
                if self.active {
                    return self.refresh();
                }
            }
            AppEvent::Navigate(motion) => {
                let count = match motion {
                    Motion::Up(count) | Motion::Left(count) => {
                        self.row = self.row.saturating_sub(*count);
                        0
                    }
                    Motion::Down(count)
                    | Motion::Right(count)
                    | Motion::NextItem(count)
                    | Motion::EndItem(count) => *count,
                    Motion::Top => {
                        self.row = 0;
                        0
                    }
                    Motion::Bottom => {
                        self.row = self.max_row;
                        0
                    }
                };
                self.row = self.row.saturating_add(count).min(self.max_row);
            }
            AppEvent::Goto(value) if self.subview == SecondarySubview::Registers => {
                return self.goto_register(value);
            }
            _ => {}
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::VirtualAddress;
    use minemu_runtime::ExecutionInspection;

    use super::SecondaryWidget;
    use crate::tui::{event::AppEvent, input::Motion, widget::TuiWidget};

    #[test]
    fn narrow_register_format_omits_decimal_values() {
        let widget = SecondaryWidget {
            execution: Some(ExecutionInspection {
                registers: [0; 16],
                cpsr: 0,
                spsr: 0,
                instruction_address: VirtualAddress::new(0),
                instruction_bytes: Vec::new(),
                instruction_error: None,
            }),
            ..SecondaryWidget::default()
        };
        let text = widget.registers(false);
        assert!(!text.contains("decimal"));
        assert!(!text.contains("          0"));
    }

    #[test]
    fn register_scroll_is_clamped_to_the_last_full_page() {
        let mut widget = SecondaryWidget {
            max_row: 4,
            ..SecondaryWidget::default()
        };
        widget.update(&AppEvent::Navigate(Motion::Down(100)));
        assert_eq!(widget.row, 4);
        widget.update(&AppEvent::Navigate(Motion::Bottom));
        assert_eq!(widget.row, 4);
    }
}
