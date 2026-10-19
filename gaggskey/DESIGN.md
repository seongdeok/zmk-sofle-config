# GaggsKey — 설계 문서

Logitech GaggsKey 같은 PC 앱으로 Sofle(ZMK) 키보드의 **상태를 실시간으로 보여주고**, PC에서 **커맨드를 보내** 제어하는 기능.

## 1. 대상 환경

| 항목 | 값 |
|---|---|
| 펌웨어 | ZMK v0.3 (Zephyr 3.5), nice!nano v2 ×2, split |
| 역할 | 왼쪽 = central (호스트 연결), 오른쪽 = peripheral |
| 기존 기능 | nice_oled, EC11 엔코더, ZMK Studio (USB UART + BLE), 마우스 |
| 호스트 | Arch Linux (Wayland/Hyprland), macOS도 나중에 지원 |

## 2. 통신 채널 결정

| 후보 | 판단 |
|---|---|
| ZMK Studio RPC 확장 | v0.3에는 사용자 정의 subsystem을 추가할 수 없고 push 이벤트도 없음. **키맵 편집 용도로만** 계속 사용 |
| CDC-ACM 시리얼 | USB에서만 동작하고 Studio UART와 겹침. 기각 |
| **벤더 HID (USB) + 커스텀 GATT (BLE)** | 채택. 드라이버 불필요, hidraw/BlueZ로 바로 접근 가능 |

- **USB**: ZMK가 쓰는 `HID_0`과 별도로 **두 번째 HID 인터페이스 `HID_1`**을 둠 (usage page `0xFF0B`).
  - kbd→host: interrupt IN
  - host→kbd: `SET_REPORT(Output)` 제어 전송. OUT 엔드포인트를 추가하지 않아서 키보드 인터페이스에 영향이 없음
- **BLE**: HOG에 report를 추가하면 ZMK 본체 패치가 필요하고 재페어링 문제도 생김. 그래서 **독립된 GATT 서비스**(UUID `5f0b0001-…`)를 쓰고, 특성 하나에 write와 notify를 둠. 암호화 권한이므로 본딩된 호스트만 접근 가능.

## 3. 구조

```
┌──────────── 왼쪽 반쪽 (central) ─────────────┐
│ gaggskey/firmware  (Zephyr module)       │
│  core.c                                     │
│   ├ ZMK event listener → 30ms debounce →    │
│   │   STATE push (모든 transport)            │
│   ├ rx msgq → system workqueue → dispatcher │
│   └ HELLO / GET_STATE / CMD 처리             │
│  transport_usb.c  (HID_1, 32B report)       │
│  transport_ble.c  (GATT notify/write)       │
└──────────────▲──────────────────────────────┘
               │ 32-byte frames
┌──────────────┴──── Host ────────────────────┐
│ [단계1] gkctl.py   : hidraw CLI (stdlib)    │
│ [단계3] gaggskeyd      : 데몬 (Rust, hidapi+bluez)│
│           └ $XDG_RUNTIME_DIR/gaggskeyd.sock    │
│ [단계4] GUI (Tauri) · waybar · 앱별 프로필    │
└─────────────────────────────────────────────┘
```

### 스레딩 규칙 (펌웨어)
- transport 콜백(USB 스택, BT RX 스레드)에서는 `gk_rx()`로 프레임을 **복사해서 큐에 넣기만** 함
- 실제 처리는 `k_work`를 통해 **system workqueue**에서 함. ZMK behavior가 실행되는 것과 같은 컨텍스트라서 keymap/ble/endpoint API를 호출해도 안전함
- push는 `k_work_delayable`로 묶음. `layer_to` 하나가 deactivate N번과 activate 1번 이벤트를 일으키는데, 이걸 STATE 하나로 합침
- USB IN 엔드포인트는 호스트가 hidraw를 열었을 때만 폴링됨. 아무도 열지 않았으면 5ms 기다린 뒤 drop해서 workqueue를 막지 않음

## 4. 프로토콜 v1

정의 원본: [`firmware/include/gaggskey/proto.h`](firmware/include/gaggskey/proto.h)

```
[0] 0x5F magic  [1] ver=1  [2] type  [3] seq  [4] len  [5..31] payload
```

| type | 방향 | 내용 |
|---|---|---|
| `0x01 HELLO` | ↔ | 응답: proto, caps 비트, layer 수, BT 프로필 수, 키보드 이름 |
| `0x02 GET_STATE` | → | 응답 `STATE` |
| `0x03 STATE` | ← | 응답(seq 포함) 또는 push(seq=0) |
| `0x10 CMD` | → | `[cmd][args]` |
| `0x11 ACK` | ← | `[cmd][int8 status(-errno)][data]` |
| `0x7F ERROR` | ← | 잘못된 프레임 |

**STATE (15 bytes)**: `layer_state u32`(layer id 비트마스크), `highest_layer`, `default_layer`, `battery_central`, `battery_periph`(0xFF=모름), `transport`(0 USB/1 BLE), `ble_profile`, `ble_connected_mask`, `wpm`, `hid_indicators`(Caps/Num), `activity`(active/idle/sleep), `flags`(usb_powered, periph_connected, profile_open, profile_connected)

**CMD**

| id | 이름 | 인자 |
|---|---|---|
| 01–04 | layer to/on/off/toggle | layer id |
| 05 | layer name | layer id → ACK data = 이름 |
| 10–13 | BT select/next/prev/clear | idx |
| 20–21 | output select/toggle | 0=USB 1=BLE |
| 7E/7F | reset / bootloader | 가드 `B0 07` (실수 방지). `CONFIG_ZMK_GAGGSKEY_DANGEROUS_CMDS=n`으로 아예 끌 수 있음 |

호환성 규칙: 필드는 **끝에만 추가**하고 `len`으로 구분함. 의미가 바뀌는 변경이 있을 때만 `ver`를 올림.

## 5. 빌드 연동

- 모듈은 왼쪽(central) 빌드에만 넣음: `-DZMK_EXTRA_MODULES=…/gaggskey/firmware -DEXTRA_CONF_FILE=…/gaggskey_central.conf`
- `gaggskey_central.conf`: `CONFIG_ZMK_GAGGSKEY=y`, `CONFIG_USB_HID_DEVICE_COUNT=2`, `CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING=y`
- 오른쪽 반쪽과 `config/sofle.conf`는 수정하지 않음
- 로컬 빌드: `gaggskey/build.sh [left|right|all]` (docker `zmk-dev-arm:3.5`)
- GitHub Actions(`build.yaml`)에 넣는 건 TODO. 왼쪽 항목의 cmake-args에 모듈 경로를 넘겨야 함

측정값 (왼쪽, GaggsKey 포함): FLASH 342,836 B (42.3%), RAM 137,004 B (52.3%)

## 6. 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| 1 | 펌웨어 모듈 (USB+BLE transport, 상태/커맨드), `gkctl` CLI | **구현됨, 빌드 통과, 실기기 검증 전** |
| 2 | 실기기 검증: USB 레이어 push/커맨드, BLE GATT (bleak으로 테스트) | |
| 3 | `gaggskeyd` 데몬 (Rust): USB/BLE 자동 선택, 재연결, 소켓 API, waybar 출력 | |
| 4 | Tauri GUI: Sofle 레이아웃과 현재 레이어 키 라벨(keymap 파싱 또는 Studio RPC), 배터리, BT 프로필, 출력 토글 | |
| 5 | 앱별 자동 레이어 (Hyprland 활성 창), 배터리 부족 알림, 레이어 OSD | |
| 6 | OLED에 호스트 알림 표시 (nice_oled 위젯 확장), 앱 안에서 키맵 편집 (Studio RPC) | |

## 7. 리스크

- **ZMK Studio와 동시 사용**: 서로 다른 인터페이스/서비스라서 충돌은 없음. Studio가 레이어를 바꿔도 이벤트가 그대로 push됨.
- **BLE MTU**: 32B 프레임에는 ATT MTU 35 이상이 필요함. BlueZ는 보통 협상하지만, 실패하면 notify가 실패함. 2단계에서 확인 필요.
- **macOS**: 벤더 HID 장치는 IOHIDManager로 접근 가능함 (권한 프롬프트가 뜰 수 있음). BLE GATT는 CoreBluetooth로 처리.
- **USB 인터페이스 추가로 디스크립터가 바뀜**: 호스트가 장치를 다시 열거함. 문제가 생길 가능성은 낮음.
