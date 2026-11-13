#include "minemu/conformance.h"
#include "minemu/platform.h"
#include "minemu/trap.h"

static volatile uint32_t irq_count;
static volatile uint32_t irq_order[2];

extern uint64_t minemu_systick_exact_128(uint32_t control);
extern uint32_t minemu_systick_exact_64(uint32_t control);

struct minemu_trap_frame *minemu_irq_dispatch(struct minemu_trap_frame *frame) {
    uint32_t cpsr;
    __asm__ volatile("mrs %0, cpsr" : "=r"(cpsr));
    MINEMU_REQUIRE((cpsr & UINT32_C(0x80)) != 0U, 22);
    uint32_t source = (uint32_t)frame->exception_id;
    MINEMU_REQUIRE(irq_count < 2U, 20);
    MINEMU_REQUIRE(source == MINEMU_IRQ_UART0 || source == MINEMU_IRQ_UART1, 21);
    irq_order[irq_count++] = source;
    if (source == MINEMU_IRQ_UART0) {
        (void)MINEMU_UART0->rx_data;
    } else {
        (void)MINEMU_UART1->rx_data;
    }
    MINEMU_INTERRUPT->eoi = source;
    return frame;
}

static void wait_uart(volatile struct minemu_uart_regs *uart) {
    while ((uart->status & MINEMU_UART_STATUS_RX_READY) == 0U) {
    }
}

static uint32_t claim(void) {
    return MINEMU_INTERRUPT->claim;
}

static void finish(uint32_t source) {
    if (source == MINEMU_IRQ_SYSTICK) {
        MINEMU_SYSTICK->ack = MINEMU_SYSTICK_ACK;
    } else if (source == MINEMU_IRQ_UART0) {
        (void)MINEMU_UART0->rx_data;
    } else if (source == MINEMU_IRQ_UART1) {
        (void)MINEMU_UART1->rx_data;
    }
    MINEMU_INTERRUPT->eoi = source;
}

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    __asm__ volatile("cpsid i" ::: "memory");

    MINEMU_SYSTICK->period = 128;
    uint64_t samples = minemu_systick_exact_128(
        MINEMU_SYSTICK_CONTROL_ENABLE | MINEMU_SYSTICK_CONTROL_IRQ_ENABLE);
    MINEMU_REQUIRE((uint32_t)samples == 0U, 1);
    MINEMU_REQUIRE((uint32_t)(samples >> 32) == MINEMU_SYSTICK_STATUS_PENDING, 13);

    wait_uart(MINEMU_UART0);
    wait_uart(MINEMU_UART1);
    MINEMU_UART0->control = MINEMU_UART_CONTROL_RX_IRQ_ENABLE;
    MINEMU_UART1->control = MINEMU_UART_CONTROL_RX_IRQ_ENABLE;
    MINEMU_INTERRUPT->enable = UINT32_C(0x7);

    MINEMU_REQUIRE(claim() == MINEMU_IRQ_SYSTICK, 2);
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_SYSTICK, 3);
    finish(MINEMU_IRQ_SYSTICK);

    MINEMU_INTERRUPT->priority_uart0 = 10;
    MINEMU_INTERRUPT->priority_uart1 = 5;
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_UART1, 4);
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_UART1, 5);
    finish(MINEMU_IRQ_UART1);
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_UART0, 6);
    finish(MINEMU_IRQ_UART0);

    wait_uart(MINEMU_UART0);
    wait_uart(MINEMU_UART1);
    MINEMU_INTERRUPT->priority_uart0 = 7;
    MINEMU_INTERRUPT->priority_uart1 = 7;
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_UART0, 7);
    finish(MINEMU_IRQ_UART0);
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_UART1, 8);
    finish(MINEMU_IRQ_UART1);
    MINEMU_REQUIRE(claim() == MINEMU_IRQ_NONE, 9);

    MINEMU_SYSTICK->period = 64;
    uint32_t periodic_samples = minemu_systick_exact_64(
        MINEMU_SYSTICK_CONTROL_ENABLE | MINEMU_SYSTICK_CONTROL_PERIODIC);
    MINEMU_REQUIRE(periodic_samples == UINT32_C(0x01000100), 14);
    MINEMU_SYSTICK->ack = MINEMU_SYSTICK_ACK;

    wait_uart(MINEMU_UART0);
    wait_uart(MINEMU_UART1);
    MINEMU_INTERRUPT->priority_uart0 = 9;
    MINEMU_INTERRUPT->priority_uart1 = 3;
    __asm__ volatile("cpsie i" ::: "memory");
    while (irq_count < 2U) {
    }
    __asm__ volatile("cpsid i" ::: "memory");
    MINEMU_REQUIRE(irq_order[0] == MINEMU_IRQ_UART1, 11);
    MINEMU_REQUIRE(irq_order[1] == MINEMU_IRQ_UART0, 12);

    minemu_trace_event(UINT32_C(0x20050001));
    minemu_fail_stop();
}
