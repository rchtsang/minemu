use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::{FaultCause, MmuInspection, PeripheralsInspection};
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
const SECONDARY_VIEW_COUNT: usize = 4;

pub struct SecondaryWidget {
    subview: SecondarySubview,
    execution: Option<ExecutionInspection>,
    peripherals: Option<PeripheralsInspection>,
    mmu: Option<MmuInspection>,
    rows: [usize; SECONDARY_VIEW_COUNT],
    max_rows: [usize; SECONDARY_VIEW_COUNT],
    active: bool,
}

impl Default for SecondaryWidget {
    fn default() -> Self {
        Self {
            subview: SecondarySubview::Registers,
            execution: None,
            peripherals: None,
            mmu: None,
            rows: [0; SECONDARY_VIEW_COUNT],
            max_rows: [REGISTER_COUNT - 1, 0, 0, 0],
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
            SecondarySubview::System => vec![Action::RequestInspection {
                target: self.id(),
                request: RuntimeInspectionRequest::Machine(minemu_platform::InspectionRequest::Mmu),
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

    fn row(&self) -> usize {
        self.rows[self.subview.index()]
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
        lines.extend(values.into_iter().skip(self.row()).map(|(name, value)| {
            if show_decimal {
                format!("{name:<5} 0x{value:08x}  {value:>10}")
            } else {
                format!("{name:<5} 0x{value:08x}")
            }
        }));
        lines.join("\n")
    }

    fn system(&self) -> String {
        self.mmu.map_or_else(
            || "MMU:\n  snapshot unavailable".into(),
            |mmu| {
                format!(
                    "MMU:\n  enabled: {}\n  ttbr0:   0x{:08x}\n  vbar:    0x{:08x}",
                    yes_no(mmu.enabled),
                    mmu.ttbr0.get(),
                    mmu.vector_base.get()
                )
            },
        )
    }

    fn peripherals(&self) -> String {
        self.peripherals.as_ref().map_or_else(
            || "Peripherals:\n  snapshot unavailable".into(),
            |p| {
                let latest_trace = p.trace.last().map_or_else(
                    || "  latest:  none".to_string(),
                    |event| {
                        format!(
                            "  tick:    0x{:016x}\n  value:   0x{:08x}",
                            event.tick, event.value
                        )
                    },
                );
                let block = format_block(&p.block);
                format!(
                    "UART0:\n  status:  0x{:08x}\n  control: 0x{:08x}\n  rx:      {}\n  rx irq:  {}\n  tx:      {} bytes\n\
UART1:\n  status:  0x{:08x}\n  control: 0x{:08x}\n  rx:      {}\n  rx irq:  {}\n  tx:      {} bytes\n\
SysTick:\n  period:  {}\n  control: 0x{:08x}\n  status:  0x{:08x}\n\
{}\n\
RNG:\n  state:   0x{:08x}\n\
Trace:\n  events:  {}\n{}",
                    p.uart0.status,
                    p.uart0.control,
                    p.uart0.rx_queued,
                    enabled_disabled(p.uart0.rx_irq_enabled),
                    p.uart0.tx_history.len(),
                    p.uart1.status,
                    p.uart1.control,
                    p.uart1.rx_queued,
                    enabled_disabled(p.uart1.rx_irq_enabled),
                    p.uart1.tx_history.len(),
                    p.systick.period,
                    p.systick.control,
                    p.systick.status,
                    block,
                    p.rng.state,
                    p.trace.len(),
                    latest_trace
                )
            },
        )
    }

    fn pending(&self) -> String {
        let interrupts = self.peripherals.as_ref().map_or_else(
            || "Interrupts:\n  snapshot unavailable".into(),
            |p| {
                format!(
                    "Interrupts:\n  pending: 0x{:08x}\n  enabled: 0x{:08x}\n  claim:   {}\n  priorities:\n    systick: {}\n    uart0:   {}\n    uart1:   {}\n    block:   {}",
                    p.interrupts.pending,
                    p.interrupts.enabled,
                    p.interrupts.claim.map_or_else(
                        || "none".into(),
                        |claim| format!("{} ({claim})", interrupt_name(claim))
                    ),
                    p.interrupts.priorities[0],
                    p.interrupts.priorities[1],
                    p.interrupts.priorities[2],
                    p.interrupts.priorities[3],
                )
            },
        );
        let fault = self.mmu.map_or_else(
            || "Fault:\n  snapshot unavailable".into(),
            |mmu| match (mmu.last_fault_address, mmu.last_fault_status) {
                (Some(address), Some(status)) => format!(
                    "Fault:\n  address: 0x{address:08x}\n  status:  0x{:08x}\n  cause:   {}\n  origin:  {}\n  access:  {}",
                    status.raw(),
                    fault_cause(status.cause()),
                    if status.from_user() { "user" } else { "supervisor" },
                    if status.is_fetch() {
                        "fetch"
                    } else if status.is_write() {
                        "write"
                    } else {
                        "read"
                    }
                ),
                _ => "Fault:\n  none".into(),
            },
        );
        format!("{interrupts}\n{fault}")
    }

    fn goto_register(&mut self, value: &str) -> Vec<Action> {
        let row = match value.to_ascii_lowercase().as_str() {
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
        self.rows[SecondarySubview::Registers.index()] = row;
        Vec::new()
    }
}

fn format_block(block: &minemu_platform::BlockInspection) -> String {
    format!(
        "Block:\n  lba:         {}\n  sectors:     {}\n  dma:         0x{:08x}\n  control:     0x{:08x}\n  status:      0x{:08x}\n  error:       0x{:08x}\n  staged unit: {}\n  active unit: {}\n  unit 0:\n    media: {}\n    dirty: {}\n  unit 1:\n    media: {}\n    dirty: {}",
        block.lba,
        block.sector_count,
        block.dma_address,
        block.control,
        block.status,
        block.error,
        block.unit,
        block
            .active_unit
            .map_or_else(|| "none".into(), |unit| unit.to_string()),
        if block.units[0].media_attached {
            "attached"
        } else {
            "none"
        },
        block.units[0].dirty_sector_count,
        if block.units[1].media_attached {
            "attached"
        } else {
            "none"
        },
        block.units[1].dirty_sector_count,
    )
}

impl TuiWidget for SecondaryWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Secondary
    }

    fn visible(&self, view: View) -> bool {
        view == View::Inspect
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let (name, text, content_length, sticky_header_rows) = match self.subview {
            SecondarySubview::Registers => (
                "registers",
                self.registers(area.width >= 35),
                REGISTER_COUNT,
                1,
            ),
            SecondarySubview::System => {
                let text = self.system();
                let length = text.lines().count();
                ("system", text, length, 0)
            }
            SecondarySubview::Peripherals => {
                let text = self.peripherals();
                let length = text.lines().count();
                ("peripherals", text, length, 0)
            }
            SecondarySubview::Pending => {
                let text = self.pending();
                let length = text.lines().count();
                ("pending", text, length, 0)
            }
        };
        let viewport = usize::from(area.height.saturating_sub(2))
            .saturating_sub(sticky_header_rows)
            .max(1);
        let index = self.subview.index();
        self.max_rows[index] = content_length.saturating_sub(viewport);
        self.rows[index] = self.rows[index].min(self.max_rows[index]);
        let row = self.rows[index];
        let scroll = if sticky_header_rows == 0 {
            row as u16
        } else {
            0
        };
        frame.render_widget(
            Paragraph::new(text).scroll((scroll, 0)).block(pane_block(
                format!("[^s] secondary ({name})"),
                context.focused == self.id(),
                area.width,
            )),
            area,
        );
        render_scrollbar(frame, area, content_length, viewport, row);
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
                if self.active {
                    return self.refresh();
                }
            }
            AppEvent::Navigate(motion) => {
                let index = self.subview.index();
                let count = match motion {
                    Motion::Up(count) | Motion::Left(count) => {
                        self.rows[index] = self.rows[index].saturating_sub(*count);
                        0
                    }
                    Motion::Down(count)
                    | Motion::Right(count)
                    | Motion::NextItem(count)
                    | Motion::EndItem(count) => *count,
                    Motion::Top => {
                        self.rows[index] = 0;
                        0
                    }
                    Motion::Bottom => {
                        self.rows[index] = self.max_rows[index];
                        0
                    }
                };
                self.rows[index] = self.rows[index]
                    .saturating_add(count)
                    .min(self.max_rows[index]);
            }
            AppEvent::Goto(value) if self.subview == SecondarySubview::Registers => {
                return self.goto_register(value);
            }
            _ => {}
        }
        Vec::new()
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn enabled_disabled(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

fn interrupt_name(source: u32) -> &'static str {
    match source {
        0 => "systick",
        1 => "uart0",
        2 => "uart1",
        3 => "block",
        _ => "unknown",
    }
}

fn fault_cause(cause: Option<FaultCause>) -> &'static str {
    match cause {
        Some(FaultCause::Translation) => "translation",
        Some(FaultCause::ReadProtection) => "read protection",
        Some(FaultCause::WriteProtection) => "write protection",
        Some(FaultCause::ExecuteProtection) => "execute protection",
        Some(FaultCause::DeviceAccess) => "device access",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{BlockInspection, BlockUnitInspection, VirtualAddress};
    use minemu_runtime::ExecutionInspection;

    use super::{SecondaryWidget, format_block};
    use crate::tui::{event::AppEvent, input::Motion, types::SecondarySubview, widget::TuiWidget};

    #[test]
    fn narrow_register_format_omits_decimal_values() {
        let widget = SecondaryWidget {
            execution: Some(ExecutionInspection {
                registers: [0; 16],
                cpsr: 0,
                spsr: 0,
                mmu_enabled: false,
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
        let mut widget = SecondaryWidget::default();
        let index = SecondarySubview::Registers.index();
        widget.max_rows[index] = 4;
        widget.update(&AppEvent::Navigate(Motion::Down(100)));
        assert_eq!(widget.rows[index], 4);
        widget.update(&AppEvent::Navigate(Motion::Bottom));
        assert_eq!(widget.rows[index], 4);
    }

    #[test]
    fn secondary_views_keep_independent_rows() {
        let mut widget = SecondaryWidget {
            subview: SecondarySubview::System,
            rows: [1, 2, 3, 4],
            max_rows: [10; 4],
            ..SecondaryWidget::default()
        };
        widget.update(&AppEvent::Navigate(Motion::Up(1)));
        assert_eq!(widget.rows, [1, 1, 3, 4]);
    }

    #[test]
    fn block_format_keeps_shared_and_per_unit_state_distinct() {
        let text = format_block(&BlockInspection {
            lba: 7,
            sector_count: 2,
            dma_address: 0x4000_0200,
            control: 1,
            status: 1,
            error: 0,
            unit: 1,
            active_unit: Some(0),
            units: [
                BlockUnitInspection {
                    dirty_sector_count: 3,
                    media_attached: true,
                },
                BlockUnitInspection {
                    dirty_sector_count: 0,
                    media_attached: false,
                },
            ],
        });
        assert!(text.contains("staged unit: 1"));
        assert!(text.contains("active unit: 0"));
        assert!(text.contains("unit 0:\n    media: attached\n    dirty: 3"));
        assert!(text.contains("unit 1:\n    media: none\n    dirty: 0"));
        assert_eq!(text.matches("status:").count(), 1);
    }
}
