/*
 * USB transport: a second, vendor-defined HID interface (HID_1).
 *
 * Host->kbd uses HID SET_REPORT(Output) on the control pipe, so no extra
 * interrupt OUT endpoint is needed (Linux hidraw write() falls back to
 * SET_REPORT when the interface has no OUT endpoint). Kbd->host uses the
 * interrupt IN endpoint.
 *
 * Requires CONFIG_USB_HID_DEVICE_COUNT >= 2 (ZMK itself owns HID_0).
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/kernel.h>
#include <zephyr/init.h>
#include <zephyr/device.h>
#include <zephyr/usb/usb_device.h>
#include <zephyr/usb/class/usb_hid.h>
#include <zephyr/logging/log.h>

#include <zmk/usb.h>

#include "op_internal.h"

LOG_MODULE_DECLARE(optionsplus, CONFIG_ZMK_OPTIONSPLUS_LOG_LEVEL);

BUILD_ASSERT(CONFIG_USB_HID_DEVICE_COUNT >= 2,
             "Options+ USB transport needs CONFIG_USB_HID_DEVICE_COUNT=2");

#define HID_DEV_NAME CONFIG_USB_HID_DEVICE_NAME "_1"

static const uint8_t op_report_desc[] = {
    0x06, (OP_HID_USAGE_PAGE & 0xFF), (OP_HID_USAGE_PAGE >> 8), /* Usage Page (vendor) */
    0x09, OP_HID_USAGE,                                         /* Usage */
    0xA1, 0x01,                                                 /* Collection (Application) */
    0x09, 0x02,                                                 /*   Usage (in) */
    0x15, 0x00,                                                 /*   Logical Min 0 */
    0x26, 0xFF, 0x00,                                           /*   Logical Max 255 */
    0x75, 0x08,                                                 /*   Report Size 8 */
    0x95, OP_FRAME_SIZE,                                        /*   Report Count */
    0x81, 0x02,                                                 /*   Input (Data,Var,Abs) */
    0x09, 0x03,                                                 /*   Usage (out) */
    0x15, 0x00,                                                 /*   Logical Min 0 */
    0x26, 0xFF, 0x00,                                           /*   Logical Max 255 */
    0x75, 0x08,                                                 /*   Report Size 8 */
    0x95, OP_FRAME_SIZE,                                        /*   Report Count */
    0x91, 0x02,                                                 /*   Output (Data,Var,Abs) */
    0xC0,                                                       /* End Collection */
};

static const struct device *op_hid_dev;
static K_SEM_DEFINE(op_in_sem, 1, 1);

static void op_in_ready(const struct device *dev) { k_sem_give(&op_in_sem); }

static int op_set_report(const struct device *dev, struct usb_setup_packet *setup, int32_t *len,
                         uint8_t **data) {
    /* wValue high byte = report type; 0x02 = Output */
    if ((setup->wValue >> 8) != 0x02) {
        return -ENOTSUP;
    }
    op_rx(OP_SRC_USB, *data, *len);
    return 0;
}

static int op_get_report(const struct device *dev, struct usb_setup_packet *setup, int32_t *len,
                         uint8_t **data) {
    return -ENOTSUP;
}

static const struct hid_ops op_hid_ops = {
    .int_in_ready = op_in_ready,
    .set_report = op_set_report,
    .get_report = op_get_report,
};

int op_usb_send(const uint8_t frame[OP_FRAME_SIZE]) {
    if (!op_hid_dev || zmk_usb_get_status() != USB_DC_CONFIGURED) {
        return -ENODEV;
    }

    /*
     * Linux only polls the IN endpoint while hidraw is open. If nobody is
     * listening the previous report is still pending; drop instead of
     * blocking the system work queue.
     */
    if (k_sem_take(&op_in_sem, K_MSEC(CONFIG_ZMK_OPTIONSPLUS_USB_TX_TIMEOUT_MS)) != 0) {
        return -EBUSY;
    }

    int err = hid_int_ep_write(op_hid_dev, frame, OP_FRAME_SIZE, NULL);
    if (err) {
        k_sem_give(&op_in_sem);
        LOG_DBG("usb tx failed %d", err);
    }
    return err;
}

static int op_usb_init(void) {
    op_hid_dev = device_get_binding(HID_DEV_NAME);
    if (!op_hid_dev) {
        LOG_ERR("%s not found", HID_DEV_NAME);
        return -ENODEV;
    }

    usb_hid_register_device(op_hid_dev, op_report_desc, sizeof(op_report_desc), &op_hid_ops);
    return usb_hid_init(op_hid_dev);
}

/* After ZMK's own HID_0 registration (CONFIG_ZMK_USB_HID_INIT_PRIORITY). */
SYS_INIT(op_usb_init, APPLICATION, CONFIG_ZMK_OPTIONSPLUS_USB_INIT_PRIORITY);
