/*
 * GaggsKey wire protocol (v1)
 *
 * Single source of truth shared by firmware and host tools.
 * Every frame is exactly GK_FRAME_SIZE bytes, little-endian.
 *
 *   [0] magic   GK_MAGIC
 *   [1] version GK_PROTO_VERSION
 *   [2] type    enum gk_msg_type
 *   [3] seq     host-chosen; echoed in replies, 0 for unsolicited pushes
 *   [4] len     payload length (<= GK_PAYLOAD_MAX)
 *   [5..]       payload
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stdint.h>

#define GK_FRAME_SIZE 32
#define GK_HEADER_SIZE 5
#define GK_PAYLOAD_MAX (GK_FRAME_SIZE - GK_HEADER_SIZE)

#define GK_MAGIC 0x5F
#define GK_PROTO_VERSION 1

/* USB: vendor HID usage page / usage used to find the interface */
#define GK_HID_USAGE_PAGE 0xFF0B
#define GK_HID_USAGE 0x01

/* BLE: custom GATT service, 128-bit UUIDs */
#define GK_BT_SERVICE_UUID "5f0b0001-6761-6767-736b-657900000000"
#define GK_BT_CHRC_UUID "5f0b0002-6761-6767-736b-657900000000"

enum gk_msg_type {
    GK_MSG_HELLO = 0x01,     /* host->kbd: empty. kbd->host: struct gk_hello */
    GK_MSG_GET_STATE = 0x02, /* host->kbd: empty. reply: GK_MSG_STATE */
    GK_MSG_STATE = 0x03,     /* kbd->host: struct gk_state (reply or push) */
    GK_MSG_CMD = 0x10,       /* host->kbd: [cmd][args...] */
    GK_MSG_ACK = 0x11,       /* kbd->host: [cmd][int8 status][data...] */
    GK_MSG_ERROR = 0x7F,     /* kbd->host: [int8 status] (bad frame) */
};

enum gk_cmd {
    GK_CMD_LAYER_TO = 0x01,         /* [layer_id] */
    GK_CMD_LAYER_ACTIVATE = 0x02,   /* [layer_id] */
    GK_CMD_LAYER_DEACTIVATE = 0x03, /* [layer_id] */
    GK_CMD_LAYER_TOGGLE = 0x04,     /* [layer_id] */
    GK_CMD_LAYER_NAME = 0x05,       /* [layer_id] -> ACK data: name (no NUL) */

    GK_CMD_BT_SELECT = 0x10, /* [profile] */
    GK_CMD_BT_NEXT = 0x11,
    GK_CMD_BT_PREV = 0x12,
    GK_CMD_BT_CLEAR = 0x13, /* clear current profile bond */

    GK_CMD_OUT_SELECT = 0x20, /* [0=USB, 1=BLE] */
    GK_CMD_OUT_TOGGLE = 0x21,

    /* keymap introspection (read-only) */
    GK_CMD_KEYMAP_INFO = 0x30,    /* -> [key_count][layer_count][behavior_count][layout_keys] */
    GK_CMD_BEHAVIOR_NAME = 0x31,  /* [idx] -> behavior device name (no NUL) */
    GK_CMD_KEYMAP_GET = 0x32,     /* [layer_id][start_pos] -> [n] n x ([bidx][uvar p1][uvar p2])
                                   * bidx 0xFF = unknown behavior. uvar = LEB128 u32. */
    GK_CMD_LAYOUT_GET = 0x33,     /* [pos] -> int16 w,h,x,y,r,rx,ry (1/100 key units,
                                   * r in 1/100 degree) of the selected physical layout */

    GK_CMD_RESET = 0x7E,      /* [0xB0][0x07] guard bytes */
    GK_CMD_BOOTLOADER = 0x7F, /* [0xB0][0x07] guard bytes */
};

#define GK_GUARD0 0xB0
#define GK_GUARD1 0x07

/* gk_hello.caps */
#define GK_CAP_USB (1u << 0)
#define GK_CAP_BLE (1u << 1)
#define GK_CAP_SPLIT (1u << 2)
#define GK_CAP_PERIPH_BATTERY (1u << 3)
#define GK_CAP_WPM (1u << 4)
#define GK_CAP_DANGEROUS (1u << 5) /* reset/bootloader commands allowed */

struct gk_hello {
    uint8_t proto_version;
    uint16_t caps;
    uint8_t layer_count;
    uint8_t ble_profile_count;
    char name[GK_PAYLOAD_MAX - 5]; /* keyboard name, NUL padded */
} __attribute__((packed));

/* gk_state.flags */
#define GK_FLAG_USB_POWERED (1u << 0)
#define GK_FLAG_PERIPH_CONNECTED (1u << 1)
#define GK_FLAG_BLE_PROFILE_OPEN (1u << 2)      /* active profile has no bond */
#define GK_FLAG_BLE_PROFILE_CONNECTED (1u << 3) /* active profile connected */

#define GK_BATTERY_UNKNOWN 0xFF

struct gk_state {
    uint32_t layer_state;    /* bitmask by layer id */
    uint8_t highest_layer;   /* layer id */
    uint8_t default_layer;   /* layer id */
    uint8_t battery_central; /* %, or GK_BATTERY_UNKNOWN */
    uint8_t battery_periph;  /* %, or GK_BATTERY_UNKNOWN */
    uint8_t transport;       /* 0=USB 1=BLE (selected output) */
    uint8_t ble_profile;     /* active profile index */
    uint8_t ble_connected_mask; /* bit i = profile i connected */
    uint8_t wpm;
    uint8_t hid_indicators; /* bit0 NumLock, bit1 CapsLock, bit2 ScrollLock */
    uint8_t activity;       /* 0=active 1=idle 2=sleep */
    uint8_t flags;          /* GK_FLAG_* */
} __attribute__((packed));

_Static_assert(sizeof(struct gk_hello) <= GK_PAYLOAD_MAX, "hello too big");
_Static_assert(sizeof(struct gk_state) <= GK_PAYLOAD_MAX, "state too big");
