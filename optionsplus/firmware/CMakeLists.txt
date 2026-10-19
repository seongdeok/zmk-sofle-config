if(CONFIG_ZMK_OPTIONSPLUS)
  # Built into the ZMK app target so ZMK's include paths/events are available.
  zephyr_include_directories(include)
  target_sources(app PRIVATE src/core.c)
  target_sources_ifdef(CONFIG_ZMK_OPTIONSPLUS_USB app PRIVATE src/transport_usb.c)
  target_sources_ifdef(CONFIG_ZMK_OPTIONSPLUS_BLE app PRIVATE src/transport_ble.c)
endif()
