/*
 * BLE transport: custom GATT service with one characteristic.
 *   write / write-without-response : host -> kbd frames
 *   notify                         : kbd -> host frames
 *
 * Encrypted permissions, so only bonded hosts can talk to it.
 * Needs ATT MTU >= GK_FRAME_SIZE + 3 (ZMK/Linux negotiate 65+).
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/kernel.h>
#include <zephyr/bluetooth/bluetooth.h>
#include <zephyr/bluetooth/conn.h>
#include <zephyr/bluetooth/gatt.h>
#include <zephyr/bluetooth/uuid.h>
#include <zephyr/logging/log.h>

#include "gk_internal.h"

LOG_MODULE_DECLARE(gaggskey, CONFIG_ZMK_GAGGSKEY_LOG_LEVEL);

/* 5f0b0001-6761-6767-736b-657900000000 ("gaggskey") */
#define GK_SVC_UUID_VAL                                                                            \
    BT_UUID_128_ENCODE(0x5f0b0001, 0x6761, 0x6767, 0x736b, 0x657900000000)
#define GK_CHRC_UUID_VAL                                                                           \
    BT_UUID_128_ENCODE(0x5f0b0002, 0x6761, 0x6767, 0x736b, 0x657900000000)

static bool notify_enabled;

static void gk_ccc_changed(const struct bt_gatt_attr *attr, uint16_t value) {
    notify_enabled = (value == BT_GATT_CCC_NOTIFY);
    LOG_INF("BLE notifications %s", notify_enabled ? "on" : "off");
    if (notify_enabled) {
        gk_request_push();
    }
}

static ssize_t gk_write(struct bt_conn *conn, const struct bt_gatt_attr *attr, const void *buf,
                        uint16_t len, uint16_t offset, uint8_t flags) {
    if (offset != 0) {
        return BT_GATT_ERR(BT_ATT_ERR_INVALID_OFFSET);
    }
    if (len < GK_HEADER_SIZE || len > GK_FRAME_SIZE) {
        return BT_GATT_ERR(BT_ATT_ERR_INVALID_ATTRIBUTE_LEN);
    }
    gk_rx(GK_SRC_BLE, buf, len);
    return len;
}

BT_GATT_SERVICE_DEFINE(gk_svc, BT_GATT_PRIMARY_SERVICE(BT_UUID_DECLARE_128(GK_SVC_UUID_VAL)),
                       BT_GATT_CHARACTERISTIC(BT_UUID_DECLARE_128(GK_CHRC_UUID_VAL),
                                              BT_GATT_CHRC_WRITE |
                                                  BT_GATT_CHRC_WRITE_WITHOUT_RESP |
                                                  BT_GATT_CHRC_NOTIFY,
                                              BT_GATT_PERM_WRITE_ENCRYPT, NULL, gk_write, NULL),
                       BT_GATT_CCC(gk_ccc_changed,
                                   BT_GATT_PERM_READ_ENCRYPT | BT_GATT_PERM_WRITE_ENCRYPT));

int gk_ble_send(const uint8_t frame[GK_FRAME_SIZE]) {
    if (!notify_enabled) {
        return -ENOTCONN;
    }
    /* conn = NULL: notify every connection that subscribed via CCC. */
    int err = bt_gatt_notify(NULL, &gk_svc.attrs[1], frame, GK_FRAME_SIZE);
    if (err) {
        LOG_DBG("ble notify failed %d", err);
    }
    return err;
}
