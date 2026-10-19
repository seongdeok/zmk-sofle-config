/*
 * Sofle Options+ wire protocol (v1)
 *
 * Single source of truth shared by firmware and host tools.
 * Every frame is exactly OP_FRAME_SIZE bytes, little-endian.
 *
 *   [0] magic   OP_MAGIC
 *   [1] version OP_PROTO_VERSION
 *   [2] type    enum op_msg_type
 *   [3] seq     host-chosen; echoed in replies, 0 for unsolicited pushes
 *   [4] len     payload length (<= OP_PAYLOAD_MAX)
 *   [5..]       payload
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stdint.h>

#define OP_FRAME_SIZE 32
#define OP_HEADER_SIZE 5
#define OP_PAYLOAD_MAX (OP_FRAME_SIZE - OP_HEADER_SIZE)

#define OP_MAGIC 0x5F
#define OP_PROTO_VERSION 1

/* USB: vendor HID usage page / usage used to find the interface */
#define OP_HID_USAGE_PAGE 0xFF0B
#define OP_HID_USAGE 0x01

/* BLE: custom GATT service, 128-bit UUIDs */
#define OP_BT_SERVICE_UUID "5f0b0001-6f70-7469-6f6e-73706c757300"
#define OP_BT_CHRC_UUID "5f0b0002-6f70-7469-6f6e-73706c757300"

enum op_msg_type {
    OP_MSG_HELLO = 0x01,     /* host->kbd: empty. kbd->host: struct op_hello */
    OP_MSG_GET_STATE = 0x02, /* host->kbd: empty. reply: OP_MSG_STATE */
    OP_MSG_STATE = 0x03,     /* kbd->host: struct op_state (reply or push) */
    OP_MSG_CMD = 0x10,       /* host->kbd: [cmd][args...] */
    OP_MSG_ACK = 0x11,       /* kbd->host: [cmd][int8 status][data...] */
    OP_MSG_ERROR = 0x7F,     /* kbd->host: [int8 status] (bad frame) */
};

enum op_cmd {
    OP_CMD_LAYER_TO = 0x01,         /* [layer_id] */
    OP_CMD_LAYER_ACTIVATE = 0x02,   /* [layer_id] */
    OP_CMD_LAYER_DEACTIVATE = 0x03, /* [layer_id] */
    OP_CMD_LAYER_TOGGLE = 0x04,     /* [layer_id] */
    OP_CMD_LAYER_NAME = 0x05,       /* [layer_id] -> ACK data: name (no NUL) */

    OP_CMD_BT_SELECT = 0x10, /* [profile] */
    OP_CMD_BT_NEXT = 0x11,
    OP_CMD_BT_PREV = 0x12,
    OP_CMD_BT_CLEAR = 0x13, /* clear current profile bond */

    OP_CMD_OUT_SELECT = 0x20, /* [0=USB, 1=BLE] */
    OP_CMD_OUT_TOGGLE = 0x21,

    OP_CMD_RESET = 0x7E,      /* [0xB0][0x07] guard bytes */
    OP_CMD_BOOTLOADER = 0x7F, /* [0xB0][0x07] guard bytes */
};

#define OP_GUARD0 0xB0
#define OP_GUARD1 0x07

/* op_hello.caps */
#define OP_CAP_USB (1u << 0)
#define OP_CAP_BLE (1u << 1)
#define OP_CAP_SPLIT (1u << 2)
#define OP_CAP_PERIPH_BATTERY (1u << 3)
#define OP_CAP_WPM (1u << 4)
#define OP_CAP_DANGEROUS (1u << 5) /* reset/bootloader commands allowed */

struct op_hello {
    uint8_t proto_version;
    uint16_t caps;
    uint8_t layer_count;
    uint8_t ble_profile_count;
    char name[OP_PAYLOAD_MAX - 5]; /* keyboard name, NUL padded */
} __attribute__((packed));

/* op_state.flags */
#define OP_FLAG_USB_POWERED (1u << 0)
#define OP_FLAG_PERIPH_CONNECTED (1u << 1)
#define OP_FLAG_BLE_PROFILE_OPEN (1u << 2)      /* active profile has no bond */
#define OP_FLAG_BLE_PROFILE_CONNECTED (1u << 3) /* active profile connected */

#define OP_BATTERY_UNKNOWN 0xFF

struct op_state {
    uint32_t layer_state;    /* bitmask by layer id */
    uint8_t highest_layer;   /* layer id */
    uint8_t default_layer;   /* layer id */
    uint8_t battery_central; /* %, or OP_BATTERY_UNKNOWN */
    uint8_t battery_periph;  /* %, or OP_BATTERY_UNKNOWN */
    uint8_t transport;       /* 0=USB 1=BLE (selected output) */
    uint8_t ble_profile;     /* active profile index */
    uint8_t ble_connected_mask; /* bit i = profile i connected */
    uint8_t wpm;
    uint8_t hid_indicators; /* bit0 NumLock, bit1 CapsLock, bit2 ScrollLock */
    uint8_t activity;       /* 0=active 1=idle 2=sleep */
    uint8_t flags;          /* OP_FLAG_* */
} __attribute__((packed));

_Static_assert(sizeof(struct op_hello) <= OP_PAYLOAD_MAX, "hello too big");
_Static_assert(sizeof(struct op_state) <= OP_PAYLOAD_MAX, "state too big");
