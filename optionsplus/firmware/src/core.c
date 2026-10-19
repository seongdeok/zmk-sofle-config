/*
 * Sofle Options+ core: state cache, event listeners, command dispatcher.
 *
 * Threading: transports hand raw frames to op_rx() (ISR / BT RX / USB
 * context). Frames are queued and processed on the system work queue,
 * which is the same context ZMK behaviors run in, so calling keymap/ble/
 * endpoint APIs from there is safe.
 *
 * SPDX-License-Identifier: MIT
 */

#include <string.h>

#include <zephyr/kernel.h>
#include <zephyr/init.h>
#include <zephyr/sys/reboot.h>
#include <zephyr/logging/log.h>

#include <zmk/event_manager.h>
#include <zmk/keymap.h>
#include <zmk/endpoints.h>
#include <zmk/activity.h>
#include <zmk/usb.h>
#include <zmk/events/layer_state_changed.h>
#include <zmk/events/activity_state_changed.h>
#include <zmk/events/endpoint_changed.h>

#if IS_ENABLED(CONFIG_ZMK_BLE)
#include <zmk/ble.h>
#include <zmk/events/ble_active_profile_changed.h>
#endif
#if IS_ENABLED(CONFIG_ZMK_BATTERY_REPORTING)
#include <zmk/battery.h>
#include <zmk/events/battery_state_changed.h>
#endif
#if IS_ENABLED(CONFIG_ZMK_WPM)
#include <zmk/wpm.h>
#include <zmk/events/wpm_state_changed.h>
#endif
#if IS_ENABLED(CONFIG_ZMK_HID_INDICATORS)
#include <zmk/hid_indicators.h>
#include <zmk/events/hid_indicators_changed.h>
#endif
#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE)
#include <zephyr/bluetooth/conn.h>
#endif
#if IS_ENABLED(CONFIG_ZMK_USB)
#include <zmk/events/usb_conn_state_changed.h>
#endif

#include "op_internal.h"

LOG_MODULE_REGISTER(optionsplus, CONFIG_ZMK_OPTIONSPLUS_LOG_LEVEL);

#if IS_ENABLED(CONFIG_ZMK_BLE)
#define BLE_PROFILES ZMK_BLE_PROFILE_COUNT
#else
#define BLE_PROFILES 0
#endif

/* ---------------------------------------------------------------- state */

static uint8_t periph_battery = OP_BATTERY_UNKNOWN;
static bool periph_connected;

static void fill_state(struct op_state *s) {
    memset(s, 0, sizeof(*s));
    s->layer_state = zmk_keymap_layer_state();
    s->highest_layer = zmk_keymap_layer_index_to_id(zmk_keymap_highest_layer_active());
    s->default_layer = zmk_keymap_layer_default();

#if IS_ENABLED(CONFIG_ZMK_BATTERY_REPORTING)
    s->battery_central = zmk_battery_state_of_charge();
#else
    s->battery_central = OP_BATTERY_UNKNOWN;
#endif
    s->battery_periph = periph_battery;

    s->transport = zmk_endpoints_selected().transport == ZMK_TRANSPORT_BLE ? 1 : 0;

#if IS_ENABLED(CONFIG_ZMK_BLE)
    s->ble_profile = zmk_ble_active_profile_index();
    for (int i = 0; i < BLE_PROFILES && i < 8; i++) {
        if (zmk_ble_profile_is_connected(i)) {
            s->ble_connected_mask |= BIT(i);
        }
    }
    if (zmk_ble_active_profile_is_open()) {
        s->flags |= OP_FLAG_BLE_PROFILE_OPEN;
    }
    if (zmk_ble_active_profile_is_connected()) {
        s->flags |= OP_FLAG_BLE_PROFILE_CONNECTED;
    }
#endif

#if IS_ENABLED(CONFIG_ZMK_WPM)
    int wpm = zmk_wpm_get_state();
    s->wpm = wpm > 255 ? 255 : (uint8_t)wpm;
#endif
#if IS_ENABLED(CONFIG_ZMK_HID_INDICATORS)
    s->hid_indicators = (uint8_t)zmk_hid_indicators_get_current_profile();
#endif
    s->activity = (uint8_t)zmk_activity_get_state();

#if IS_ENABLED(CONFIG_ZMK_USB)
    if (zmk_usb_is_powered()) {
        s->flags |= OP_FLAG_USB_POWERED;
    }
#endif
    if (periph_connected) {
        s->flags |= OP_FLAG_PERIPH_CONNECTED;
    }
}

/* ------------------------------------------------------------- framing */

static void frame_init(uint8_t f[OP_FRAME_SIZE], uint8_t type, uint8_t seq) {
    memset(f, 0, OP_FRAME_SIZE);
    f[0] = OP_MAGIC;
    f[1] = OP_PROTO_VERSION;
    f[2] = type;
    f[3] = seq;
    f[4] = 0;
}

static void frame_put(uint8_t f[OP_FRAME_SIZE], const void *data, size_t len) {
    size_t room = OP_PAYLOAD_MAX - f[4];
    if (len > room) {
        len = room;
    }
    memcpy(&f[OP_HEADER_SIZE + f[4]], data, len);
    f[4] += len;
}

static void send(enum op_src dst, const uint8_t f[OP_FRAME_SIZE]) {
#if IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_USB)
    if (dst == OP_SRC_USB || dst == OP_SRC_ALL) {
        op_usb_send(f);
    }
#endif
#if IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_BLE)
    if (dst == OP_SRC_BLE || dst == OP_SRC_ALL) {
        op_ble_send(f);
    }
#endif
}

static void send_state(enum op_src dst, uint8_t seq) {
    uint8_t f[OP_FRAME_SIZE];
    struct op_state s;

    fill_state(&s);
    frame_init(f, OP_MSG_STATE, seq);
    frame_put(f, &s, sizeof(s));
    send(dst, f);
}

static void send_hello(enum op_src dst, uint8_t seq) {
    uint8_t f[OP_FRAME_SIZE];
    struct op_hello h = {0};
    uint16_t caps = 0;

    if (IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_USB)) {
        caps |= OP_CAP_USB;
    }
    if (IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_BLE)) {
        caps |= OP_CAP_BLE;
    }
    if (IS_ENABLED(CONFIG_ZMK_SPLIT)) {
        caps |= OP_CAP_SPLIT;
    }
    if (IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)) {
        caps |= OP_CAP_PERIPH_BATTERY;
    }
    if (IS_ENABLED(CONFIG_ZMK_WPM)) {
        caps |= OP_CAP_WPM;
    }
    if (IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_DANGEROUS_CMDS)) {
        caps |= OP_CAP_DANGEROUS;
    }

    h.proto_version = OP_PROTO_VERSION;
    h.caps = caps;
    h.layer_count = ZMK_KEYMAP_LAYERS_LEN;
    h.ble_profile_count = BLE_PROFILES;
    strncpy(h.name, CONFIG_ZMK_KEYBOARD_NAME, sizeof(h.name));

    frame_init(f, OP_MSG_HELLO, seq);
    frame_put(f, &h, sizeof(h));
    send(dst, f);
}

static void send_ack(enum op_src dst, uint8_t seq, uint8_t cmd, int status, const void *data,
                     size_t len) {
    uint8_t f[OP_FRAME_SIZE];
    int8_t st = status < -128 ? -128 : (int8_t)status;

    frame_init(f, OP_MSG_ACK, seq);
    frame_put(f, &cmd, 1);
    frame_put(f, &st, 1);
    if (data && len) {
        frame_put(f, data, len);
    }
    send(dst, f);
}

static void send_error(enum op_src dst, uint8_t seq, int status) {
    uint8_t f[OP_FRAME_SIZE];
    int8_t st = (int8_t)status;

    frame_init(f, OP_MSG_ERROR, seq);
    frame_put(f, &st, 1);
    send(dst, f);
}

/* ------------------------------------------------------------ commands */

static bool guard_ok(const uint8_t *a, uint8_t n) {
    return n >= 3 && a[1] == OP_GUARD0 && a[2] == OP_GUARD1;
}

static bool layer_ok(uint8_t id) { return id < ZMK_KEYMAP_LAYERS_LEN; }

static void handle_cmd(enum op_src src, uint8_t seq, const uint8_t *p, uint8_t n) {
    if (n < 1) {
        send_error(src, seq, -EINVAL);
        return;
    }

    uint8_t cmd = p[0];
    int rc = -ENOTSUP;

    switch (cmd) {
    case OP_CMD_LAYER_TO:
    case OP_CMD_LAYER_ACTIVATE:
    case OP_CMD_LAYER_DEACTIVATE:
    case OP_CMD_LAYER_TOGGLE:
        if (n < 2 || !layer_ok(p[1])) {
            rc = -EINVAL;
            break;
        }
        rc = cmd == OP_CMD_LAYER_TO           ? zmk_keymap_layer_to(p[1])
             : cmd == OP_CMD_LAYER_ACTIVATE   ? zmk_keymap_layer_activate(p[1])
             : cmd == OP_CMD_LAYER_DEACTIVATE ? zmk_keymap_layer_deactivate(p[1])
                                              : zmk_keymap_layer_toggle(p[1]);
        break;

    case OP_CMD_LAYER_NAME: {
        if (n < 2 || !layer_ok(p[1])) {
            rc = -EINVAL;
            break;
        }
        const char *name = zmk_keymap_layer_name(p[1]);
        if (!name) {
            name = "";
        }
        send_ack(src, seq, cmd, 0, name, strnlen(name, OP_PAYLOAD_MAX - 2));
        return;
    }

#if IS_ENABLED(CONFIG_ZMK_BLE)
    case OP_CMD_BT_SELECT:
        rc = n < 2 ? -EINVAL : zmk_ble_prof_select(p[1]);
        break;
    case OP_CMD_BT_NEXT:
        rc = zmk_ble_prof_next();
        break;
    case OP_CMD_BT_PREV:
        rc = zmk_ble_prof_prev();
        break;
    case OP_CMD_BT_CLEAR:
        zmk_ble_clear_bonds();
        rc = 0;
        break;
#endif

    case OP_CMD_OUT_SELECT:
        rc = n < 2 ? -EINVAL
                   : zmk_endpoints_select_transport(p[1] ? ZMK_TRANSPORT_BLE : ZMK_TRANSPORT_USB);
        break;
    case OP_CMD_OUT_TOGGLE:
        rc = zmk_endpoints_toggle_transport();
        break;

#if IS_ENABLED(CONFIG_ZMK_OPTIONSPLUS_DANGEROUS_CMDS)
    case OP_CMD_RESET:
    case OP_CMD_BOOTLOADER:
        if (!guard_ok(p, n)) {
            rc = -EPERM;
            break;
        }
        send_ack(src, seq, cmd, 0, NULL, 0);
        k_msleep(50); /* let the ACK leave */
        sys_reboot(cmd == OP_CMD_BOOTLOADER ? 0x57 /* RST_UF2 */ : SYS_REBOOT_WARM);
        return;
#endif

    default:
        break;
    }

    send_ack(src, seq, cmd, rc, NULL, 0);
}

/* ------------------------------------------------------------- RX path */

struct rx_item {
    uint8_t src;
    uint8_t data[OP_FRAME_SIZE];
};

K_MSGQ_DEFINE(op_rx_q, sizeof(struct rx_item), 8, 4);

static void rx_work_cb(struct k_work *w) {
    struct rx_item it;

    while (k_msgq_get(&op_rx_q, &it, K_NO_WAIT) == 0) {
        const uint8_t *f = it.data;
        enum op_src src = it.src;

        if (f[0] != OP_MAGIC || f[1] != OP_PROTO_VERSION || f[4] > OP_PAYLOAD_MAX) {
            LOG_WRN("bad frame magic=%02x ver=%02x len=%u", f[0], f[1], f[4]);
            send_error(src, f[3], -EBADMSG);
            continue;
        }

        switch (f[2]) {
        case OP_MSG_HELLO:
            send_hello(src, f[3]);
            break;
        case OP_MSG_GET_STATE:
            send_state(src, f[3]);
            break;
        case OP_MSG_CMD:
            handle_cmd(src, f[3], &f[OP_HEADER_SIZE], f[4]);
            break;
        default:
            send_error(src, f[3], -ENOTSUP);
            break;
        }
    }
}

static K_WORK_DEFINE(op_rx_work, rx_work_cb);

void op_rx(enum op_src src, const uint8_t *data, size_t len) {
    struct rx_item it = {.src = src};

    /* Some hosts prepend a zero report id; tolerate it. */
    if (len == OP_FRAME_SIZE + 1 && data[0] == 0 && data[1] == OP_MAGIC) {
        data++;
        len--;
    }
    memcpy(it.data, data, MIN(len, sizeof(it.data)));

    if (k_msgq_put(&op_rx_q, &it, K_NO_WAIT) != 0) {
        LOG_WRN("rx queue full, dropping frame");
        return;
    }
    k_work_submit(&op_rx_work);
}

/* ------------------------------------------------------------ TX pushes */

static void push_work_cb(struct k_work *w) { send_state(OP_SRC_ALL, 0); }

static K_WORK_DELAYABLE_DEFINE(op_push_work, push_work_cb);

void op_request_push(void) {
    /* coalesce bursts (e.g. layer_to = N deactivate + 1 activate events) */
    k_work_reschedule(&op_push_work, K_MSEC(CONFIG_ZMK_OPTIONSPLUS_PUSH_DEBOUNCE_MS));
}

static int op_listener(const zmk_event_t *eh) {
#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)
    const struct zmk_peripheral_battery_state_changed *pb =
        as_zmk_peripheral_battery_state_changed(eh);
    if (pb && pb->source == 0) {
        periph_battery = pb->state_of_charge;
    }
#endif
    op_request_push();
    return ZMK_EV_EVENT_BUBBLE;
}

ZMK_LISTENER(optionsplus, op_listener);
ZMK_SUBSCRIPTION(optionsplus, zmk_layer_state_changed);
ZMK_SUBSCRIPTION(optionsplus, zmk_activity_state_changed);
ZMK_SUBSCRIPTION(optionsplus, zmk_endpoint_changed);
#if IS_ENABLED(CONFIG_ZMK_BLE)
ZMK_SUBSCRIPTION(optionsplus, zmk_ble_active_profile_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_BATTERY_REPORTING)
ZMK_SUBSCRIPTION(optionsplus, zmk_battery_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)
ZMK_SUBSCRIPTION(optionsplus, zmk_peripheral_battery_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_WPM)
ZMK_SUBSCRIPTION(optionsplus, zmk_wpm_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_HID_INDICATORS)
ZMK_SUBSCRIPTION(optionsplus, zmk_hid_indicators_changed);
#endif

#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE)
/*
 * zmk_split_peripheral_status_changed is only raised on the peripheral side.
 * On the central, hosts connect to us as BLE peripheral, while the link to the
 * other half is the one where we are the BLE central role.
 */
static bool is_split_link(struct bt_conn *conn) {
    struct bt_conn_info info;
    return bt_conn_get_info(conn, &info) == 0 && info.role == BT_CONN_ROLE_CENTRAL;
}

static void op_connected(struct bt_conn *conn, uint8_t err) {
    if (!err && is_split_link(conn)) {
        periph_connected = true;
        op_request_push();
    }
}

static void op_disconnected(struct bt_conn *conn, uint8_t reason) {
    if (is_split_link(conn)) {
        periph_connected = false;
        periph_battery = OP_BATTERY_UNKNOWN;
        op_request_push();
    }
}

BT_CONN_CB_DEFINE(optionsplus_conn_cb) = {
    .connected = op_connected,
    .disconnected = op_disconnected,
};
#endif
