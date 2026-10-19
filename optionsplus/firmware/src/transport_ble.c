/*
 * BLE transport: custom GATT service with one characteristic.
 *   write / write-without-response : host -> kbd frames
 *   notify                         : kbd -> host frames
 *
 * Encrypted permissions, so only bonded hosts can talk to it.
 * Needs ATT MTU >= OP_FRAME_SIZE + 3 (ZMK/Linux negotiate 65+).
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/kernel.h>
#include <zephyr/bluetooth/bluetooth.h>
#include <zephyr/bluetooth/conn.h>
#include <zephyr/bluetooth/gatt.h>
#include <zephyr/bluetooth/uuid.h>
#include <zephyr/logging/log.h>

#include "op_internal.h"

LOG_MODULE_DECLARE(optionsplus, CONFIG_ZMK_OPTIONSPLUS_LOG_LEVEL);

/* 5f0b0001-6f70-7469-6f6e-73706c757300 ("options plus") */
#define OP_SVC_UUID_VAL                                                                            \
    BT_UUID_128_ENCODE(0x5f0b0001, 0x6f70, 0x7469, 0x6f6e, 0x73706c757300)
#define OP_CHRC_UUID_VAL                                                                           \
    BT_UUID_128_ENCODE(0x5f0b0002, 0x6f70, 0x7469, 0x6f6e, 0x73706c757300)

static bool notify_enabled;

static void op_ccc_changed(const struct bt_gatt_attr *attr, uint16_t value) {
    notify_enabled = (value == BT_GATT_CCC_NOTIFY);
    LOG_INF("BLE notifications %s", notify_enabled ? "on" : "off");
    if (notify_enabled) {
        op_request_push();
    }
}

static ssize_t op_write(struct bt_conn *conn, const struct bt_gatt_attr *attr, const void *buf,
                        uint16_t len, uint16_t offset, uint8_t flags) {
    if (offset != 0) {
        return BT_GATT_ERR(BT_ATT_ERR_INVALID_OFFSET);
    }
    if (len < OP_HEADER_SIZE || len > OP_FRAME_SIZE) {
        return BT_GATT_ERR(BT_ATT_ERR_INVALID_ATTRIBUTE_LEN);
    }
    op_rx(OP_SRC_BLE, buf, len);
    return len;
}

BT_GATT_SERVICE_DEFINE(op_svc, BT_GATT_PRIMARY_SERVICE(BT_UUID_DECLARE_128(OP_SVC_UUID_VAL)),
                       BT_GATT_CHARACTERISTIC(BT_UUID_DECLARE_128(OP_CHRC_UUID_VAL),
                                              BT_GATT_CHRC_WRITE |
                                                  BT_GATT_CHRC_WRITE_WITHOUT_RESP |
                                                  BT_GATT_CHRC_NOTIFY,
                                              BT_GATT_PERM_WRITE_ENCRYPT, NULL, op_write, NULL),
                       BT_GATT_CCC(op_ccc_changed,
                                   BT_GATT_PERM_READ_ENCRYPT | BT_GATT_PERM_WRITE_ENCRYPT));

int op_ble_send(const uint8_t frame[OP_FRAME_SIZE]) {
    if (!notify_enabled) {
        return -ENOTCONN;
    }
    /* conn = NULL: notify every connection that subscribed via CCC. */
    int err = bt_gatt_notify(NULL, &op_svc.attrs[1], frame, OP_FRAME_SIZE);
    if (err) {
        LOG_DBG("ble notify failed %d", err);
    }
    return err;
}
