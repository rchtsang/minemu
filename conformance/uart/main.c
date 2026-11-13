#include "minemu/conformance.h"
#include "minemu/platform.h"

static uint32_t receive(volatile struct minemu_uart_regs *uart) {
    while ((uart->status & MINEMU_UART_STATUS_RX_READY) == 0U) {
    }
    return uart->rx_data;
}

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    MINEMU_REQUIRE((MINEMU_UART0->status & MINEMU_UART_STATUS_TX_READY) != 0U, 1);
    MINEMU_REQUIRE((MINEMU_UART1->status & MINEMU_UART_STATUS_TX_READY) != 0U, 2);

    uint32_t uart0 = receive(MINEMU_UART0);
    uint32_t uart1 = receive(MINEMU_UART1);
    MINEMU_REQUIRE(uart0 == 'A', 3);
    MINEMU_REQUIRE(uart1 == 'B', 4);
    MINEMU_REQUIRE((MINEMU_UART0->status & MINEMU_UART_STATUS_RX_READY) == 0U, 5);
    MINEMU_REQUIRE((MINEMU_UART1->status & MINEMU_UART_STATUS_RX_READY) == 0U, 6);

    MINEMU_UART0->tx_data = uart0;
    MINEMU_UART1->tx_data = uart1;
    minemu_trace_event(UINT32_C(0x20010001));
    minemu_fail_stop();
}
