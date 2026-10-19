/*
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stddef.h>
#include <stdint.h>
#include <optionsplus/proto.h>

enum op_src {
    OP_SRC_USB = 0,
    OP_SRC_BLE = 1,
    OP_SRC_ALL = 0xFF,
};

/* Called by transports (any context, ISR-safe). Frame is copied. */
void op_rx(enum op_src src, const uint8_t *data, size_t len);

/* Implemented by transports. Called from the system work queue only. */
int op_usb_send(const uint8_t frame[OP_FRAME_SIZE]);
int op_ble_send(const uint8_t frame[OP_FRAME_SIZE]);

/* Transport -> core: a host (re)attached, push fresh state. */
void op_request_push(void);
