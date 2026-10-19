/*
 * GaggsKey core: state cache, event listeners, command dispatcher.
 *
 * Threading: transports hand raw frames to gk_rx() (ISR / BT RX / USB
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
#include <zmk/matrix.h>
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

#include <drivers/behavior.h>
#include <zmk/physical_layouts.h>

#include "gk_internal.h"

LOG_MODULE_REGISTER(gaggskey, CONFIG_ZMK_GAGGSKEY_LOG_LEVEL);

#if IS_ENABLED(CONFIG_ZMK_BLE)
#define BLE_PROFILES ZMK_BLE_PROFILE_COUNT
#else
#define BLE_PROFILES 0
#endif

/* ---------------------------------------------------------------- state */

static uint8_t periph_battery = GK_BATTERY_UNKNOWN;
static bool periph_connected;

static void fill_state(struct gk_state *s) {
    memset(s, 0, sizeof(*s));
    s->layer_state = zmk_keymap_layer_state();
    s->highest_layer = zmk_keymap_layer_index_to_id(zmk_keymap_highest_layer_active());
    s->default_layer = zmk_keymap_layer_default();

#if IS_ENABLED(CONFIG_ZMK_BATTERY_REPORTING)
    s->battery_central = zmk_battery_state_of_charge();
#else
    s->battery_central = GK_BATTERY_UNKNOWN;
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
        s->flags |= GK_FLAG_BLE_PROFILE_OPEN;
    }
    if (zmk_ble_active_profile_is_connected()) {
        s->flags |= GK_FLAG_BLE_PROFILE_CONNECTED;
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
        s->flags |= GK_FLAG_USB_POWERED;
    }
#endif
    if (periph_connected) {
        s->flags |= GK_FLAG_PERIPH_CONNECTED;
    }
}

/* ------------------------------------------------------------- framing */

static void frame_init(uint8_t f[GK_FRAME_SIZE], uint8_t type, uint8_t seq) {
    memset(f, 0, GK_FRAME_SIZE);
    f[0] = GK_MAGIC;
    f[1] = GK_PROTO_VERSION;
    f[2] = type;
    f[3] = seq;
    f[4] = 0;
}

static void frame_put(uint8_t f[GK_FRAME_SIZE], const void *data, size_t len) {
    size_t room = GK_PAYLOAD_MAX - f[4];
    if (len > room) {
        len = room;
    }
    memcpy(&f[GK_HEADER_SIZE + f[4]], data, len);
    f[4] += len;
}

static void send(enum gk_src dst, const uint8_t f[GK_FRAME_SIZE]) {
#if IS_ENABLED(CONFIG_ZMK_GAGGSKEY_USB)
    if (dst == GK_SRC_USB || dst == GK_SRC_ALL) {
        gk_usb_send(f);
    }
#endif
#if IS_ENABLED(CONFIG_ZMK_GAGGSKEY_BLE)
    if (dst == GK_SRC_BLE || dst == GK_SRC_ALL) {
        gk_ble_send(f);
    }
#endif
}

static void send_state(enum gk_src dst, uint8_t seq) {
    uint8_t f[GK_FRAME_SIZE];
    struct gk_state s;

    fill_state(&s);
    frame_init(f, GK_MSG_STATE, seq);
    frame_put(f, &s, sizeof(s));
    send(dst, f);
}

static void send_hello(enum gk_src dst, uint8_t seq) {
    uint8_t f[GK_FRAME_SIZE];
    struct gk_hello h = {0};
    uint16_t caps = 0;

    if (IS_ENABLED(CONFIG_ZMK_GAGGSKEY_USB)) {
        caps |= GK_CAP_USB;
    }
    if (IS_ENABLED(CONFIG_ZMK_GAGGSKEY_BLE)) {
        caps |= GK_CAP_BLE;
    }
    if (IS_ENABLED(CONFIG_ZMK_SPLIT)) {
        caps |= GK_CAP_SPLIT;
    }
    if (IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)) {
        caps |= GK_CAP_PERIPH_BATTERY;
    }
    if (IS_ENABLED(CONFIG_ZMK_WPM)) {
        caps |= GK_CAP_WPM;
    }
    if (IS_ENABLED(CONFIG_ZMK_GAGGSKEY_DANGEROUS_CMDS)) {
        caps |= GK_CAP_DANGEROUS;
    }

    h.proto_version = GK_PROTO_VERSION;
    h.caps = caps;
    h.layer_count = ZMK_KEYMAP_LAYERS_LEN;
    h.ble_profile_count = BLE_PROFILES;
    strncpy(h.name, CONFIG_ZMK_KEYBOARD_NAME, sizeof(h.name));

    frame_init(f, GK_MSG_HELLO, seq);
    frame_put(f, &h, sizeof(h));
    send(dst, f);
}

static void send_ack(enum gk_src dst, uint8_t seq, uint8_t cmd, int status, const void *data,
                     size_t len) {
    uint8_t f[GK_FRAME_SIZE];
    int8_t st = status < -128 ? -128 : (int8_t)status;

    frame_init(f, GK_MSG_ACK, seq);
    frame_put(f, &cmd, 1);
    frame_put(f, &st, 1);
    if (data && len) {
        frame_put(f, data, len);
    }
    send(dst, f);
}

static void send_error(enum gk_src dst, uint8_t seq, int status) {
    uint8_t f[GK_FRAME_SIZE];
    int8_t st = (int8_t)status;

    frame_init(f, GK_MSG_ERROR, seq);
    frame_put(f, &st, 1);
    send(dst, f);
}

/* ------------------------------------------------------------ commands */

static bool guard_ok(const uint8_t *a, uint8_t n) {
    return n >= 3 && a[1] == GK_GUARD0 && a[2] == GK_GUARD1;
}

static bool layer_ok(uint8_t id) { return id < ZMK_KEYMAP_LAYERS_LEN; }

/* ------------------------------------------------------ keymap introspection */

static uint8_t behavior_count(void) {
    int n;
    STRUCT_SECTION_COUNT(zmk_behavior_ref, &n);
    return n > 0xFE ? 0xFE : (uint8_t)n;
}

static uint8_t behavior_index(const char *dev_name) {
    uint8_t i = 0;
    if (!dev_name) {
        return 0xFF;
    }
    STRUCT_SECTION_FOREACH(zmk_behavior_ref, ref) {
        if (i >= 0xFE) {
            break;
        }
        if (ref->device && strcmp(ref->device->name, dev_name) == 0) {
            return i;
        }
        i++;
    }
    return 0xFF;
}

static const char *behavior_name_at(uint8_t idx) {
    uint8_t i = 0;
    STRUCT_SECTION_FOREACH(zmk_behavior_ref, ref) {
        if (i++ == idx) {
            return ref->device ? ref->device->name : "";
        }
    }
    return NULL;
}

static size_t put_uvar(uint8_t *out, uint32_t v) {
    size_t n = 0;
    do {
        uint8_t b = v & 0x7F;
        v >>= 7;
        out[n++] = b | (v ? 0x80 : 0);
    } while (v);
    return n;
}

static const struct zmk_physical_layout *selected_layout(void) {
    const struct zmk_physical_layout *const *list;
    size_t n = zmk_physical_layouts_get_list(&list);
    int sel = zmk_physical_layouts_get_selected();
    if (n == 0) {
        return NULL;
    }
    return list[(sel >= 0 && (size_t)sel < n) ? sel : 0];
}

/* returns bytes written into out (max GK_PAYLOAD_MAX - 2) */
static size_t keymap_get(uint8_t layer, uint8_t start, uint8_t *out) {
    const size_t room = GK_PAYLOAD_MAX - 2;
    size_t len = 1;
    uint8_t cnt = 0;

    for (uint32_t pos = start; pos < ZMK_KEYMAP_LEN; pos++) {
        const struct zmk_behavior_binding *b = zmk_keymap_get_layer_binding_at_idx(layer, pos);
        uint8_t tmp[11];
        size_t t = 0;

        tmp[t++] = b ? behavior_index(b->behavior_dev) : 0xFF;
        t += put_uvar(&tmp[t], b ? b->param1 : 0);
        t += put_uvar(&tmp[t], b ? b->param2 : 0);
        if (len + t > room) {
            break;
        }
        memcpy(&out[len], tmp, t);
        len += t;
        cnt++;
    }
    out[0] = cnt;
    return len;
}

static void handle_cmd(enum gk_src src, uint8_t seq, const uint8_t *p, uint8_t n) {
    if (n < 1) {
        send_error(src, seq, -EINVAL);
        return;
    }

    uint8_t cmd = p[0];
    int rc = -ENOTSUP;

    switch (cmd) {
    case GK_CMD_LAYER_TO:
    case GK_CMD_LAYER_ACTIVATE:
    case GK_CMD_LAYER_DEACTIVATE:
    case GK_CMD_LAYER_TOGGLE:
        if (n < 2 || !layer_ok(p[1])) {
            rc = -EINVAL;
            break;
        }
        rc = cmd == GK_CMD_LAYER_TO           ? zmk_keymap_layer_to(p[1])
             : cmd == GK_CMD_LAYER_ACTIVATE   ? zmk_keymap_layer_activate(p[1])
             : cmd == GK_CMD_LAYER_DEACTIVATE ? zmk_keymap_layer_deactivate(p[1])
                                              : zmk_keymap_layer_toggle(p[1]);
        break;

    case GK_CMD_LAYER_NAME: {
        if (n < 2 || !layer_ok(p[1])) {
            rc = -EINVAL;
            break;
        }
        const char *name = zmk_keymap_layer_name(p[1]);
        if (!name) {
            name = "";
        }
        send_ack(src, seq, cmd, 0, name, strnlen(name, GK_PAYLOAD_MAX - 2));
        return;
    }

    case GK_CMD_KEYMAP_INFO: {
        const struct zmk_physical_layout *pl = selected_layout();
        uint8_t d[4] = {ZMK_KEYMAP_LEN, ZMK_KEYMAP_LAYERS_LEN, behavior_count(),
                        pl ? MIN(pl->keys_len, 0xFF) : 0};
        send_ack(src, seq, cmd, 0, d, sizeof(d));
        return;
    }

    case GK_CMD_BEHAVIOR_NAME: {
        const char *name = n < 2 ? NULL : behavior_name_at(p[1]);
        if (!name) {
            rc = -EINVAL;
            break;
        }
        send_ack(src, seq, cmd, 0, name, strnlen(name, GK_PAYLOAD_MAX - 2));
        return;
    }

    case GK_CMD_KEYMAP_GET: {
        uint8_t d[GK_PAYLOAD_MAX];
        if (n < 3 || !layer_ok(p[1]) || p[2] >= ZMK_KEYMAP_LEN) {
            rc = -EINVAL;
            break;
        }
        size_t len = keymap_get(p[1], p[2], d);
        send_ack(src, seq, cmd, 0, d, len);
        return;
    }

    case GK_CMD_LAYOUT_GET: {
        const struct zmk_physical_layout *pl = selected_layout();
        if (n < 2 || !pl || p[1] >= pl->keys_len) {
            rc = -EINVAL;
            break;
        }
        const struct zmk_key_physical_attrs *k = &pl->keys[p[1]];
        int16_t d[7] = {k->width, k->height, k->x, k->y, 0, 0, 0};
#if IS_ENABLED(CONFIG_ZMK_PHYSICAL_LAYOUT_KEY_ROTATION)
        d[4] = k->r;
        d[5] = k->rx;
        d[6] = k->ry;
#endif
        send_ack(src, seq, cmd, 0, d, sizeof(d));
        return;
    }

#if IS_ENABLED(CONFIG_ZMK_BLE)
    case GK_CMD_BT_SELECT:
        rc = n < 2 ? -EINVAL : zmk_ble_prof_select(p[1]);
        break;
    case GK_CMD_BT_NEXT:
        rc = zmk_ble_prof_next();
        break;
    case GK_CMD_BT_PREV:
        rc = zmk_ble_prof_prev();
        break;
    case GK_CMD_BT_CLEAR:
        zmk_ble_clear_bonds();
        rc = 0;
        break;
#endif

    case GK_CMD_OUT_SELECT:
        rc = n < 2 ? -EINVAL
                   : zmk_endpoints_select_transport(p[1] ? ZMK_TRANSPORT_BLE : ZMK_TRANSPORT_USB);
        break;
    case GK_CMD_OUT_TOGGLE:
        rc = zmk_endpoints_toggle_transport();
        break;

#if IS_ENABLED(CONFIG_ZMK_GAGGSKEY_DANGEROUS_CMDS)
    case GK_CMD_RESET:
    case GK_CMD_BOOTLOADER:
        if (!guard_ok(p, n)) {
            rc = -EPERM;
            break;
        }
        send_ack(src, seq, cmd, 0, NULL, 0);
        k_msleep(50); /* let the ACK leave */
        sys_reboot(cmd == GK_CMD_BOOTLOADER ? 0x57 /* RST_UF2 */ : SYS_REBOOT_WARM);
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
    uint8_t data[GK_FRAME_SIZE];
};

K_MSGQ_DEFINE(gk_rx_q, sizeof(struct rx_item), 8, 4);

static void rx_work_cb(struct k_work *w) {
    struct rx_item it;

    while (k_msgq_get(&gk_rx_q, &it, K_NO_WAIT) == 0) {
        const uint8_t *f = it.data;
        enum gk_src src = it.src;

        if (f[0] != GK_MAGIC || f[1] != GK_PROTO_VERSION || f[4] > GK_PAYLOAD_MAX) {
            LOG_WRN("bad frame magic=%02x ver=%02x len=%u", f[0], f[1], f[4]);
            send_error(src, f[3], -EBADMSG);
            continue;
        }

        switch (f[2]) {
        case GK_MSG_HELLO:
            send_hello(src, f[3]);
            break;
        case GK_MSG_GET_STATE:
            send_state(src, f[3]);
            break;
        case GK_MSG_CMD:
            handle_cmd(src, f[3], &f[GK_HEADER_SIZE], f[4]);
            break;
        default:
            send_error(src, f[3], -ENOTSUP);
            break;
        }
    }
}

static K_WORK_DEFINE(gk_rx_work, rx_work_cb);

void gk_rx(enum gk_src src, const uint8_t *data, size_t len) {
    struct rx_item it = {.src = src};

    /* Some hosts prepend a zero report id; tolerate it. */
    if (len == GK_FRAME_SIZE + 1 && data[0] == 0 && data[1] == GK_MAGIC) {
        data++;
        len--;
    }
    memcpy(it.data, data, MIN(len, sizeof(it.data)));

    if (k_msgq_put(&gk_rx_q, &it, K_NO_WAIT) != 0) {
        LOG_WRN("rx queue full, dropping frame");
        return;
    }
    k_work_submit(&gk_rx_work);
}

/* ------------------------------------------------------------ TX pushes */

static void push_work_cb(struct k_work *w) { send_state(GK_SRC_ALL, 0); }

static K_WORK_DELAYABLE_DEFINE(gk_push_work, push_work_cb);

void gk_request_push(void) {
    /* coalesce bursts (e.g. layer_to = N deactivate + 1 activate events) */
    k_work_reschedule(&gk_push_work, K_MSEC(CONFIG_ZMK_GAGGSKEY_PUSH_DEBOUNCE_MS));
}

static int gk_listener(const zmk_event_t *eh) {
#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)
    const struct zmk_peripheral_battery_state_changed *pb =
        as_zmk_peripheral_battery_state_changed(eh);
    if (pb && pb->source == 0) {
        periph_battery = pb->state_of_charge;
    }
#endif
    gk_request_push();
    return ZMK_EV_EVENT_BUBBLE;
}

ZMK_LISTENER(gaggskey, gk_listener);
ZMK_SUBSCRIPTION(gaggskey, zmk_layer_state_changed);
ZMK_SUBSCRIPTION(gaggskey, zmk_activity_state_changed);
ZMK_SUBSCRIPTION(gaggskey, zmk_endpoint_changed);
#if IS_ENABLED(CONFIG_ZMK_BLE)
ZMK_SUBSCRIPTION(gaggskey, zmk_ble_active_profile_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_BATTERY_REPORTING)
ZMK_SUBSCRIPTION(gaggskey, zmk_battery_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING)
ZMK_SUBSCRIPTION(gaggskey, zmk_peripheral_battery_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_WPM)
ZMK_SUBSCRIPTION(gaggskey, zmk_wpm_state_changed);
#endif
#if IS_ENABLED(CONFIG_ZMK_HID_INDICATORS)
ZMK_SUBSCRIPTION(gaggskey, zmk_hid_indicators_changed);
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

static void gk_connected(struct bt_conn *conn, uint8_t err) {
    if (!err && is_split_link(conn)) {
        periph_connected = true;
        gk_request_push();
    }
}

static void gk_disconnected(struct bt_conn *conn, uint8_t reason) {
    if (is_split_link(conn)) {
        periph_connected = false;
        periph_battery = GK_BATTERY_UNKNOWN;
        gk_request_push();
    }
}

BT_CONN_CB_DEFINE(gaggskey_conn_cb) = {
    .connected = gk_connected,
    .disconnected = gk_disconnected,
};
#endif
