/*
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stddef.h>
#include <stdint.h>
#include <gaggskey/proto.h>

enum gk_src {
    GK_SRC_USB = 0,
    GK_SRC_BLE = 1,
    GK_SRC_ALL = 0xFF,
};

/* Called by transports (any context, ISR-safe). Frame is copied. */
void gk_rx(enum gk_src src, const uint8_t *data, size_t len);

/* Implemented by transports. Called from the system work queue only. */
int gk_usb_send(const uint8_t frame[GK_FRAME_SIZE]);
int gk_ble_send(const uint8_t frame[GK_FRAME_SIZE]);

/* Transport -> core: a host (re)attached, push fresh state. */
void gk_request_push(void);
