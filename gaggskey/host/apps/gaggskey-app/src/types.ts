// Mirrors gaggskey_core::Snapshot / gaggskey_proto::{Hello, State} (serde JSON).
export interface Hello {
  name: string;
  proto: number;
  caps: string[];
  layers: number;
  ble_profiles: number;
}

export interface State {
  layers_active: number[];
  layer: number;
  default_layer: number;
  battery: { left: number | null; right: number | null };
  output: "usb" | "ble";
  ble_profile: number;
  ble_connected: number[];
  wpm: number;
  num_lock: boolean;
  caps_lock: boolean;
  scroll_lock: boolean;
  activity: "active" | "idle" | "sleep" | "unknown";
  usb_powered: boolean;
  periph_connected: boolean;
  ble_profile_open: boolean;
  ble_profile_connected: boolean;
}

export interface Snapshot {
  connected: boolean;
  via: string | null;
  hello: Hello | null;
  layer_names: string[];
  state: State | null;
  error: string | null;
}

export type UiCommand =
  | { cmd: "layer_to" | "layer_toggle" | "bt_select"; arg: number }
  | { cmd: "out_select"; arg: "usb" | "ble" }
  | { cmd: "bt_next" | "bt_prev" | "bt_clear" | "out_toggle" | "reset" | "bootloader" };
